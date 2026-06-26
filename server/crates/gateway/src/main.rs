use shared::proto::{PlayerInput, WorldSnapshot};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let (input_tx, mut input_rx) = mpsc::channel::<PlayerInput>(4096);
    let (snapshot_tx, _) = broadcast::channel::<Arc<WorldSnapshot>>(128);

    tokio::spawn(async move {
        while let Some(input) = input_rx.recv().await {
            tracing::debug!(
                entity_id = input.entity_id,
                sequence  = input.sequence,
                "Received PlayerInput"
            );
        }
    });

    gateway::run_gateway(input_tx, snapshot_tx).await
}
