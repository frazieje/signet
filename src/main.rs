use envoy_types::pb::envoy::service::ext_proc::v3::external_processor_server::ExternalProcessorServer;
use httpsig_hyper::prelude::*;
use std::sync::Arc;
use tokio::signal::unix::{SignalKind, signal};
use tonic::transport::Server;
use tracing::info;

use signet::SignetConfig;
use signet::processor::SignetExternalProcessor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = SignetConfig::from_env();

    let addr = format!("[::]:{}", config.port).parse()?;

    let secret_key_string = std::fs::read_to_string(&config.key_path)?;
    let secret_key = SecretKey::from_pem(secret_key_string.as_str())?;

    let signet = SignetExternalProcessor {
        secret_key: Arc::new(secret_key),
    };

    let (health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<ExternalProcessorServer<SignetExternalProcessor>>()
        .await;

    let mut sigterm = signal(SignalKind::terminate())?;

    info!(port = config.port, "Signet starting");

    Server::builder()
        .add_service(health_service)
        .add_service(
            ExternalProcessorServer::new(signet)
                .max_decoding_message_size(16 * 1024 * 1024),
        )
        .serve_with_shutdown(addr, async {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT, shutting down");
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, shutting down");
                }
            }
        })
        .await?;

    Ok(())
}
