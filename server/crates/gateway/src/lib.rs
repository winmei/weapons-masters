use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use shared::proto::{PlayerInput, WorldSnapshot};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use wtransport::tls::Sha256DigestFmt;
use wtransport::{Endpoint, Identity, ServerConfig};

pub const DEFAULT_BIND_PORT: u16 = 4433;
pub const DEFAULT_WEBSOCKET_PORT: u16 = 8080;
static NEXT_WEBSOCKET_ENTITY_ID: AtomicU32 = AtomicU32::new(1_000_000);

pub async fn run_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<WorldSnapshot>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::spawn(run_websocket_gateway(
        input_tx.clone(),
        snapshot_tx.clone(),
    ));

    let identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])?;
    if let Some(certificate) = identity.certificate_chain().as_slice().first() {
        tracing::info!(
            "Browser WebTransport certificate hash bytes: {}",
            certificate.hash().fmt(Sha256DigestFmt::BytesArray)
        );
    }

    let config = ServerConfig::builder()
        .with_bind_default(DEFAULT_BIND_PORT)
        .with_identity(identity)
        .keep_alive_interval(Some(std::time::Duration::from_secs(3)))
        .build();

    let server = Endpoint::server(config)?;
    tracing::info!(
        "Gateway server started on UDP {} (WebTransport/QUIC)",
        DEFAULT_BIND_PORT
    );

    loop {
        let incoming_session = server.accept().await;
        let input_tx = input_tx.clone();
        let snapshot_rx = snapshot_tx.subscribe();

        tokio::spawn(async move {
            match incoming_session.await {
                Ok(request) => match request.accept().await {
                    Ok(connection) => {
                        tracing::info!("New connection: {:?}", connection.remote_address());
                        handle_connection(connection, input_tx, snapshot_rx).await;
                    }
                    Err(error) => {
                        tracing::warn!("Failed to accept WebTransport request: {}", error);
                    }
                },
                Err(error) => {
                    tracing::warn!("Failed to receive WebTransport session: {}", error);
                }
            }
        });
    }
}

async fn handle_connection(
    connection: wtransport::Connection,
    input_tx: mpsc::Sender<PlayerInput>,
    mut snapshot_rx: broadcast::Receiver<WorldSnapshot>,
) {
    let assigned_entity_id = non_zero_entity_id(connection.stable_id() as u32);

    loop {
        tokio::select! {
            received = connection.receive_datagram() => {
                match received {
                    Ok(datagram) => {
                        match PlayerInput::decode(datagram.payload()) {
                            Ok(mut input) => {
                                input.entity_id = assigned_entity_id;

                                if input_tx.send(input).await.is_err() {
                                    tracing::warn!("World input channel closed");
                                    break;
                                }
                            }
                            Err(error) => {
                                tracing::warn!("Invalid PlayerInput datagram: {}", error);
                            }
                        }
                    }
                    Err(error) => {
                        tracing::info!("Connection closed while receiving datagram: {}", error);
                        break;
                    }
                }
            }
            snapshot = snapshot_rx.recv() => {
                match snapshot {
                    Ok(mut snapshot) => {
                        snapshot.local_entity_id = assigned_entity_id;
                        let mut payload = Vec::with_capacity(snapshot.encoded_len());
                        if snapshot.encode(&mut payload).is_ok() {
                            if let Err(error) = connection.send_datagram(&payload) {
                                tracing::debug!("Failed to send snapshot datagram: {}", error);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("Gateway snapshot receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn non_zero_entity_id(entity_id: u32) -> u32 {
    if entity_id == 0 {
        1
    } else {
        entity_id
    }
}

async fn run_websocket_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<WorldSnapshot>,
) {
    let bind_addr = format!("0.0.0.0:{}", DEFAULT_WEBSOCKET_PORT);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!("Failed to bind WebSocket gateway on {}: {}", bind_addr, error);
            return;
        }
    };

    tracing::info!(
        "WebSocket fallback gateway started on TCP {}",
        DEFAULT_WEBSOCKET_PORT
    );

    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!("Failed to accept WebSocket TCP connection: {}", error);
                continue;
            }
        };

        let input_tx = input_tx.clone();
        let snapshot_rx = snapshot_tx.subscribe();

        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(socket) => {
                    tracing::info!("New WebSocket fallback connection: {}", remote_addr);
                    handle_websocket_connection(socket, input_tx, snapshot_rx).await;
                }
                Err(error) => {
                    tracing::warn!("Failed WebSocket handshake from {}: {}", remote_addr, error);
                }
            }
        });
    }
}

async fn handle_websocket_connection(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    input_tx: mpsc::Sender<PlayerInput>,
    mut snapshot_rx: broadcast::Receiver<WorldSnapshot>,
) {
    let (mut write, mut read) = socket.split();
    let assigned_entity_id = NEXT_WEBSOCKET_ENTITY_ID.fetch_add(1, Ordering::Relaxed);

    loop {
        tokio::select! {
            received = read.next() => {
                match received {
                    Some(Ok(WsMessage::Binary(payload))) => {
                        match PlayerInput::decode(payload.as_slice()) {
                            Ok(mut input) => {
                                input.entity_id = assigned_entity_id;
                                if input_tx.send(input).await.is_err() {
                                    tracing::warn!("World input channel closed");
                                    break;
                                }
                            }
                            Err(error) => tracing::warn!("Invalid WebSocket PlayerInput: {}", error),
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::info!("WebSocket connection closed with error: {}", error);
                        break;
                    }
                }
            }
            snapshot = snapshot_rx.recv() => {
                match snapshot {
                    Ok(mut snapshot) => {
                        snapshot.local_entity_id = assigned_entity_id;
                        let mut payload = Vec::with_capacity(snapshot.encoded_len());
                        if snapshot.encode(&mut payload).is_ok()
                            && write.send(WsMessage::Binary(payload)).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("WebSocket snapshot receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
