

# samserve

A minimal and efficient static file server designed as a **lightweight** and **feature-rich** replacement for `python3 -m http.server`.

---

## Why samserve?

If you've used `python3 -m http.server`, you know it's handy for quickly serving files over HTTP. However, it lacks:

- File upload support  
- Authentication  
- Proper handling of large files  
- Protection against directory traversal attacks  
- Modern and user-friendly directory listings  

**samserve** solves these limitations while remaining simple, fast, and easy to use.

---

## Features

- **Small binary size:**  
  The release binary is under **3 MB**, making it easy to deploy anywhere.

- **Static file serving:**  
  Serve files from any directory with proper MIME type detection.

- **Directory listing:**  
  Clean, user-friendly, modern HTML directory listings.

- **Basic Authentication:**  
  HTTP Basic Auth support to protect your files and uploads.

- **File uploads:**  
  Upload files via HTTP multipart/form-data.

- **Range requests:**  
  Efficient large file serving with HTTP range requests support.

- **Secure by default:**  
  Protects against directory traversal and unauthorized access.

- **Detailed logging:**  
  Logs requests with method, path, status, and remote address.

---

## Installation

Build from source:

```bash
cargo build --release
cp target/release/samserve /usr/local/bin/
```

---

## Usage

```bash
samserve --root ./public --port 8000 --upload --auth user:password
```

### Options:

- `--root` - Root directory to serve (default: `.`)
    
- `--ip` - IP address to bind (default: `0.0.0.0`)
    
- `--port` - Port to listen on (default: `8000`)
    
- `--upload` - Enable file upload support
    
- `--auth` - Enable Basic Auth (`username:password`)
    

---

## Example

Serve the `public` folder on port 8080 with uploads and basic auth:

```bash
samserve --root public --port 8080 --upload --auth admin:secret
```

---

## Comparison with `python3 -m http.server`

| Feature                   | python3 -m http.server | samserve               |
| ------------------------- | ---------------------- | ---------------------- |
| File upload support       | ❌                      | ✅                      |
| Basic Authentication      | ❌                      | ✅                      |
| Large file streaming      | ❌                      | ✅                      |
| Directory listing         | Basic HTML             | Modern, styled listing |
| Protection from traversal | No                     | Yes                    |
| Binary size               | Larger (Python + deps) | < 3 MB                 |

---

## License

MIT License