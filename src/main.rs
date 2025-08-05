use clap::Parser;
use hyper::Server;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use log::{error, info, warn};
use simple_logger;
use std::{convert::Infallible, net::SocketAddr};
mod args;
mod handler;
use args::Args;
use base64::{Engine as _, engine::general_purpose};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    simple_logger::SimpleLogger::new().init().unwrap();
    let args = Args::parse();
    info!("Parsed arguments...");
    info!("Root directory: {}", args.root);
    info!("Upload support: {}", args.upload);
    let base64_auth: Option<String> = match args.auth {
        Some(auth) => {
            info!("Basic Auth enabled with credentials: {}", auth);
            let encoded_string = general_purpose::STANDARD.encode(auth.as_bytes());
            Some(encoded_string)
        }
        None => {
            warn!("Basic Auth not enabled");
            None
        }
    };
    let bind_address = format!("{}:{}", args.ip, args.port);
    let addr: SocketAddr = bind_address.parse().unwrap_or_else(|_| {
        error!("Invalid address format: {}", bind_address);
        std::process::exit(1);
    });
    info!("Starting server on {}", addr);
    let root_dir = Arc::new(args.root.clone());

    let arc_base64_auth = Arc::new(base64_auth);
    let make_svc = make_service_fn(|_conn: &AddrStream| {
        let remote_addr = _conn.remote_addr();
        let root_dir = root_dir.clone();
        let arc_base64_auth = arc_base64_auth.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handler::handle_requests(
                    req,
                    remote_addr.clone(),
                    Arc::clone(&root_dir),
                    Arc::clone(&arc_base64_auth),
                    args.upload,
                )
            }))
        }
    });
    if let Err(e) = Server::bind(&addr).serve(make_svc).await {
        error!("Server Error: {}",e);
        std::process::exit(1);
    };
}
