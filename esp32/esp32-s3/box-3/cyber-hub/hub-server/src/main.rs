use anyhow::Result;
use tokio::net::TcpListener;
use tracing::{error, info};

use hub_server::ai::AiProcessor;
use hub_server::config::AppConfig;
use hub_server::gateway::handle_device_connection;
use hub_server::system::metrics::MetricsMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let config = AppConfig::from_env();
    let addr = format!("0.0.0.0:{}", config.port);

    let ai_processor = std::sync::Arc::new(AiProcessor::new(&config).await);
    let metrics = std::sync::Arc::new(MetricsMonitor::new());

    let listener = TcpListener::bind(&addr).await?;
    info!("CyberHub Server (Mac) starting on {}...", addr);

    loop {
        let (socket, addr) = listener.accept().await?;
        info!("Accepted connection from {}", addr);
        let config_clone = config.clone();
        let metrics_clone = metrics.clone();
        let ai_clone = ai_processor.clone();

        tokio::spawn(async move {
            if let Err(e) =
                handle_device_connection(socket, config_clone, metrics_clone, ai_clone).await
            {
                error!("Error handling connection from {}: {}", addr, e);
            }
        });
    }
}
