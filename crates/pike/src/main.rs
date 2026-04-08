use clap::Parser;
use pike::cdp::{CdpServer, CdpServerConfig};

#[derive(Parser)]
#[command(name = "pike", version, about = "Minimal browser engine for AI agents")]
struct Cli {
    /// Port for the CDP server (0 = auto-assign).
    #[arg(short, long, default_value = "0")]
    port: u16,

    /// Run in headless mode (no window).
    #[arg(long, default_value = "true")]
    headless: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pike=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    let server = CdpServer::new(CdpServerConfig {
        port: cli.port,
        headless: cli.headless,
    });

    let port = server.start().await.expect("failed to start CDP server");
    println!("pike CDP server running on http://127.0.0.1:{}", port);
    println!("WebSocket: ws://127.0.0.1:{}/devtools/browser", port);

    // Wait forever.
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    println!("\nshutting down");
}
