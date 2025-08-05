use clap::Parser;

/// A minimal file server with upload support and Basic Auth
#[derive(Parser, Debug)]
#[command(
    author = "3amim <3amim.3amim@gmail.com>",
    name = "samserve",
    version,
    about = "A minimal file server with upload support and Basic Auth",
    long_about = Some(
        "samserve is a tiny yet powerful static file server\n\
        It supports file uploads, HTTP basic authentication, and directory listings out of the box.\n\
        Ideal for quick sharing or development environments."
    ),
    propagate_version = true
)]
pub struct Args {
    #[arg(short, long, default_value = ".", help = "Root directory to serve files from")]
    pub root: String,

    #[arg(short, long, default_value = "0.0.0.0", help = "IP address to bind to")]
    pub ip: String,

    #[arg(short, long, default_value_t = 8000, help = "Port to listen on")]
    pub port: u16,

    #[arg(short, long, default_value = "false", help = "Enable upload support")]
    pub upload: bool,

    #[arg(short, long, help = "Enable basic authentication. Format: username:password")]
    pub auth: Option<String>,
}
