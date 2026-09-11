//! Binary entrypoint: load config from the environment and serve HTTP.

use std::net::SocketAddr;

use cursor_bridge::Config;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    let host = std::env::var("CURSOR_BRIDGE_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("CURSOR_BRIDGE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8787);

    let app = cursor_bridge::app(config);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("invalid CURSOR_BRIDGE_HOST/PORT");

    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server error");
}
