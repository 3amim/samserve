use base64::{Engine as _, engine::general_purpose};
use futures_util::TryStreamExt;
use html_escape::encode_text;
use hyper::{Body, Method, Request, Response, StatusCode, header};
use log::{error, info, warn};
use mime_guess::from_path;
use multer::Multipart;
use percent_encoding::percent_decode_str;
use std::sync::Arc;
use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};
use tokio::fs;
use tokio::fs::{File, read_dir};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio_util::io::ReaderStream;

pub async fn handle_requests(
    req: Request<Body>,
    remote_addr: std::net::SocketAddr,
    root_dir: Arc<String>,
    auth: Arc<Option<String>>,
    upload: bool,
) -> Result<Response<Body>, Infallible> {
    if let Some(base64_auth) = &*auth {
        if let Err(unauthorize) = check_basic_auth(&req, base64_auth, remote_addr) {
            return Ok(unauthorize);
        }
    }
    let uri_path = (&req).uri().path();
    if req.method() == Method::POST {
        if upload {
            return handle_upload(req, PathBuf::from(root_dir.as_str()), remote_addr).await;
        } else {
            error!(
                "Upload attempted but uploads are disabled | path: {:?} | version: {:?} | status: {} | remote: {}",
                uri_path,
                req.version(),
                StatusCode::FORBIDDEN,
                remote_addr
            );
            return Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::from("Uploads are disabled on this server"))
                .unwrap());
        }
    }
    let range_header = req
        .headers()
        .get(header::RANGE)
        .and_then(|h| h.to_str().ok());
    let response = match serve_file(uri_path, &root_dir, remote_addr, range_header).await {
        Ok(resp) => resp,
        Err(resp) => resp,
    };
    Ok(response)
}

async fn serve_file(
    request_path: &str,
    root: &str,
    remote_addr: std::net::SocketAddr,
    range_header: Option<&str>,
) -> Result<Response<Body>, Response<Body>> {
    let decoded_path = match percent_decode_str(request_path).decode_utf8() {
        Ok(path) => path,
        Err(err) => {
            error!(
                "Invalid URL path decoding | raw: {:?} | error: {} | status: {} | remote: {}",
                request_path,
                err,
                StatusCode::BAD_REQUEST,
                remote_addr
            );
            return Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Invalid path"))
                .unwrap());
        }
    };
    let mut path = PathBuf::from(root);
    for part in Path::new(&*decoded_path).components() {
        use std::path::Component::*;
        match part {
            Normal(comp) => path.push(comp),
            CurDir => {}
            RootDir => {}
            _ => {
                warn!(
                    "Directory traversal attempt blocked | input: {:?} | component: {:?} | status: {} | remote: {}",
                    decoded_path,
                    part,
                    StatusCode::FORBIDDEN,
                    remote_addr
                );
                return Err(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::from("Forbidden"))
                    .unwrap());
            }
        }
    }

    let metadata = match fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(err) => {
            error!(
                "Failed to read metadata | path: {:?} | error: {} | status: {} | remote: {}",
                path,
                err,
                StatusCode::NOT_FOUND,
                remote_addr
            );
            return Err(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap());
        }
    };

    if metadata.is_dir() {
        let index_path = path.join("index.html");
        if index_path.exists() {
            info!(
                "Serving index.html | path: {:?} | requested: {:?} | status: {} | remote: {}",
                index_path,
                request_path,
                StatusCode::OK,
                remote_addr
            );
            return stream_file(&index_path, remote_addr, range_header).await;
        } else {
            let listing = render_directory_listing(&path, request_path).await;
            match listing {
                Ok(html) => {
                    info!(
                        "Directory listing | path: {:?} | requested: {:?} | status: {} | remote: {}",
                        path,
                        request_path,
                        StatusCode::OK,
                        remote_addr
                    );
                    return Ok(Response::builder()
                        .header("Content-Type", "text/html")
                        .body(Body::from(html))
                        .unwrap());
                }
                Err(err) => {
                    error!(
                        "Error rendering directory listing | path: {:?} | error: {} | status: {} | remote: {}",
                        path,
                        err,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        remote_addr
                    );
                    return Err(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Error rendering directory listing"))
                        .unwrap());
                }
            }
        }
    }
    stream_file(&path, remote_addr, range_header).await
}

async fn stream_file(
    path: &Path,
    remote_addr: std::net::SocketAddr,
    range_header: Option<&str>,
) -> Result<Response<Body>, Response<Body>> {
    let mut file = match File::open(path).await {
        Ok(f) => f,
        Err(err) => {
            error!(
                "File open error | path: {:?} | error: {} | status: {} | remote: {}",
                path,
                err,
                StatusCode::NOT_FOUND,
                remote_addr
            );
            return Err(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap());
        }
    };

    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(err) => {
            error!(
                "Metadata error | path: {:?} | error: {} | status: {} | remote: {}",
                path,
                err,
                StatusCode::NOT_FOUND,
                remote_addr
            );
            return Err(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("File not found"))
                .unwrap());
        }
    };
    let file_size = metadata.len();
    let mime = from_path(path).first_or_octet_stream();
    if let Some(range_header) = range_header {
        if let Some((start, end)) = parse_range_header(range_header, file_size) {
            if start >= file_size || end >= file_size || start > end {
                error!(
                    "Invalid range | range: {} | file_size: {} | status: {} | remote: {}",
                    range_header,
                    file_size,
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    remote_addr
                );
                return Err(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{}", file_size))
                    .body(Body::empty())
                    .unwrap());
            }

            if let Err(err) = file.seek(SeekFrom::Start(start)).await {
                error!(
                    "Seek failed | path: {:?} | error: {} | status: {} | remote: {}",
                    path,
                    err,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    remote_addr
                );
                return Err(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Seek error"))
                    .unwrap());
            }
            let chunk_size = end - start + 1;
            let stream = ReaderStream::new(file.take(chunk_size));
            let body = Body::wrap_stream(stream);
            info!(
                "Partial content | {:?} | range: {}-{} | status: {} | remote: {}",
                path,
                start,
                end,
                StatusCode::PARTIAL_CONTENT,
                remote_addr
            );
            return Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, mime.to_string())
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, file_size),
                )
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, chunk_size.to_string())
                .body(body)
                .unwrap());
        }
    }

    let stream = ReaderStream::new(file);
    let body = Body::wrap_stream(stream);

    info!(
        "Full content | path: {:?} | status: {} | remote: {}",
        path,
        StatusCode::OK,
        remote_addr
    );
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.to_string())
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .body(body)
        .unwrap())
}

pub async fn render_directory_listing(
    path: &Path,
    request_path: &str,
) -> Result<String, std::io::Error> {
    let mut entries = read_dir(path).await?;
    let mut list_items = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let encoded_name = encode_text(&name_str);

        let metadata = entry.metadata().await?;
        let is_dir = metadata.is_dir();

        let icon = if is_dir { "📁" } else { "📄" };
        let href = if is_dir {
            format!("{}/", encoded_name)
        } else {
            encoded_name.to_string()
        };

        let item = format!(
            r#"<li><span class="icon">{}</span><a href="{}">{}</a></li>"#,
            icon, href, encoded_name
        );
        list_items.push(item);
    }

    // Upload form as last list item
    list_items.push(
        r#"
    <li>
        <form class="upload" action="." method="POST" enctype="multipart/form-data">
            <label style="display: block; margin-bottom: 0.3rem;">
                <span class="icon">📤</span> Upload a file:
            </label>
            <input type="file" name="file" required style="margin-bottom: 0.5rem;">
            <input type="submit" value="Upload">
        </form>
    </li>
    "#
        .to_string(),
    );

    let entries_html = list_items.join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Index of {}</title>
    <style>
        body {{
            font-family: sans-serif;
            background: #f8f9fa;
            color: #333;
            padding: 2rem;
        }}
        h1 {{
            font-size: 1.5rem;
            margin-bottom: 1rem;
        }}
        a {{
            color: #007bff;
            text-decoration: none;
        }}
        a:hover {{
            text-decoration: underline;
        }}
        ul {{
            list-style: none;
            padding-left: 0;
        }}
        li {{
            margin: 0.25rem 0;
        }}
        .icon {{
            display: inline-block;
            width: 1.5em;
        }}
        form.upload {{
            display: flex;
            flex-direction: column;
            background: #f0f0f0;
            padding: 0.5rem;
            border-radius: 6px;
            border: 1px solid #ccc;
            max-width: 300px;
            margin-top: 1rem;
        }}
        form.upload input[type="file"] {{
            margin-bottom: 0.5rem;
        }}
        form.upload input[type="submit"] {{
            align-self: flex-start;
            background-color: #007bff;
            color: white;
            border: none;
            padding: 0.4rem 1rem;
            border-radius: 4px;
            cursor: pointer;
        }}
        form.upload input[type="submit"]:hover {{
            background-color: #0056b3;
        }}
    </style>
</head>
<body>
    <h1>Index of {}</h1>
    <ul>
        {}
    </ul>
</body>
</html>"#,
        encode_text(request_path),
        encode_text(request_path),
        entries_html
    );

    Ok(html)
}

fn check_basic_auth(
    req: &Request<Body>,
    base64_auth: &String,
    remote_addr: std::net::SocketAddr,
) -> Result<(), Response<Body>> {
    let Some(auth_header) = req.headers().get(header::AUTHORIZATION) else {
        warn!(
            " Missing Authorization header | method: {:?} | uri: {:?} | status: {} | remote: {:?}",
            req.method(),
            req.uri(),
            StatusCode::UNAUTHORIZED,
            remote_addr
        );
        return Err(unauthorized_response());
    };

    let auth_str = auth_header.to_str().unwrap_or("");
    if !auth_str.starts_with("Basic ") {
        warn!(
            "Invalid auth scheme | got: {:?} | method: {} | status: {} | uri: {} | remote: {}",
            auth_str,
            req.method(),
            StatusCode::UNAUTHORIZED,
            req.uri(),
            remote_addr
        );
        return Err(unauthorized_response());
    }

    let encoded = (&auth_str[6..]).to_string(); // remove "Basic "

    if *base64_auth == encoded {
        Ok(())
    } else {
        let decoded = general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .unwrap();
        warn!(
            "Auth failed | method: {} | uri: {} | status: {} | provided: {:?} | remote: {}",
            req.method(),
            req.uri(),
            StatusCode::UNAUTHORIZED,
            String::from_utf8(decoded).unwrap(),
            remote_addr
        );
        Err(unauthorized_response())
    }
}

fn unauthorized_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, r#"Basic realm="Restricted""#)
        .body(Body::from("<h1><center>Unauthorized</center></h1>"))
        .unwrap()
}

pub async fn handle_upload(
    req: Request<Body>,
    root_dir: PathBuf,
    remote_addr: std::net::SocketAddr,
) -> Result<Response<Body>, Infallible> {
    let path_uri = req.uri().path().trim_start_matches("/");
    let target_dir = root_dir.join(PathBuf::from(path_uri));
    let version = (&req.version()).clone();
    if target_dir.exists() && !target_dir.is_dir() {
        error!(
            "Upload failed: target path exists and is not a directory | path: {:?} | version: {:?} | status: {} | remote: {}",
            target_dir,
            version,
            StatusCode::BAD_REQUEST,
            remote_addr
        );
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .body(Body::from("Upload path is a file"))
            .unwrap());
    }
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type.starts_with("multipart/form-data") {
        error!(
            "Bad request: expected multipart/form-data | got: {:?} | path: {:?} | version: {:?} | status: {} | remote: {}",
            content_type,
            target_dir,
            version,
            StatusCode::BAD_REQUEST,
            remote_addr
        );
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("Expected multipart/form-data"))
            .unwrap());
    }

    // parse the multipart body
    let boundary = multer::parse_boundary(content_type).unwrap_or_default();
    let mut multipart = Multipart::new(req.into_body(), boundary);

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() != Some("file") {
            continue;
        }

        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or("upload.bin".to_string());

        let safe_name = sanitize_filename::sanitize(&file_name);
        let save_path = target_dir.join(safe_name);
        let mut file = File::create(&save_path).await.unwrap();
        let mut field_data = field.into_stream();

        while let Ok(Some(chunk)) = field_data.try_next().await {
            let data = chunk;
            file.write_all(&data).await.unwrap();
        }
        info!(
            "Upload complete | path: {:?} | version: {:?} | status: {} | remote: {}",
            save_path,
            version,
            StatusCode::SEE_OTHER,
            remote_addr
        );
        return Ok(Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header("Location", ".")
            .body(Body::empty())
            .unwrap());
    }
    error!(
        "POST upload failed: no file field | target_dir: {:?} | version: {:?} | status: {} | remote: {}",
        target_dir,
        version,
        StatusCode::BAD_REQUEST,
        remote_addr
    );
    Ok(Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::from("No file field"))
        .unwrap())
}

fn parse_range_header(header: &str, file_size: u64) -> Option<(u64, u64)> {
    if !header.starts_with("bytes=") {
        return None;
    }
    let range = &header[6..];
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parts[0].parse::<u64>().ok();
    let end = parts[1].parse::<u64>().ok();

    match (start, end) {
        (Some(s), Some(e)) if s <= e => Some((s, e)),
        (Some(s), None) if s < file_size => Some((s, file_size - 1)),
        (None, Some(e)) if e != 0 => {
            let size = file_size.min(e);
            Some((file_size - size, file_size - 1))
        }
        _ => None,
    }
}
