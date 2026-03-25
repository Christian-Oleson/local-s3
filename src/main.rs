use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use local_s3::server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("local_s3=info".parse().unwrap()),
        )
        .init();

    let port: u16 = std::env::var("LOCAL_S3_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(4566);

    let data_dir = std::env::var("LOCAL_S3_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"));

    // Also support --port and --data-dir CLI args
    let args: Vec<String> = std::env::args().collect();
    let port = parse_arg(&args, "--port").unwrap_or(port);
    let data_dir = parse_arg::<String>(&args, "--data-dir")
        .map(PathBuf::from)
        .unwrap_or(data_dir);

    tracing::info!(
        "Starting local-s3 on port {port} with data dir: {}",
        data_dir.display()
    );

    if let Err(e) = server::run_server(port, data_dir).await {
        tracing::error!("Server error: {e}");
        std::process::exit(1);
    }
}

fn parse_arg<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}
