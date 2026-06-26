use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use shared::proto::{
    GameAuthPacket, LoginRequest, LoginResponse, PlayerInput, RegisterRequest, WorldSnapshot,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use wtransport::tls::Sha256DigestFmt;
use wtransport::{Endpoint, Identity, ServerConfig};

pub const DEFAULT_BIND_PORT: u16 = 4433;
pub const DEFAULT_WEBSOCKET_PORT: u16 = 8080;
/// Auth requests arrive on a dedicated port so they never reach the game input channel.
pub const DEFAULT_AUTH_PORT: u16 = 8081;

// IDs WebSocket começam em 1_000_000 para não colidir com IDs WebTransport
// baseados em connection.stable_id(), que são tipicamente pequenos.
static NEXT_WEBSOCKET_ENTITY_ID: AtomicU32 = AtomicU32::new(1_000_000);

/// Called by the game WebSocket handler when a `GameAuthPacket` is received.
/// Receives the assigned entity_id and the JWT token.
/// Validates the token, loads CharacterData from the DB, and pushes a
/// `CharacterAssignment` to the world ECS channel.
/// `None` return means the game_auth handler is not configured (dev mode).
pub type GameAuthFn = dyn Fn(u32, String) -> futures_util::future::BoxFuture<'static, ()>
    + Send
    + Sync;

/// Optional game-auth handler injected at startup.
/// When `None`, `GameAuthPacket` is silently ignored (dev/test mode).
pub struct GatewayConfig {
    pub game_auth: Option<Arc<GameAuthFn>>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self { game_auth: None }
    }
}

pub async fn run_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<WorldSnapshot>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_gateway_with_config(input_tx, snapshot_tx, GatewayConfig::default()).await
}

pub async fn run_gateway_with_config(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<WorldSnapshot>>,
    config: GatewayConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::spawn(run_websocket_gateway(
        input_tx.clone(),
        snapshot_tx.clone(),
        config.game_auth,
    ));

    let wt_identity = Identity::self_signed(["localhost", "127.0.0.1", "::1"])?;
    if let Some(certificate) = wt_identity.certificate_chain().as_slice().first() {
        tracing::info!(
            "Browser WebTransport certificate hash bytes: {}",
            certificate.hash().fmt(Sha256DigestFmt::BytesArray)
        );
    }

    let wt_config = ServerConfig::builder()
        .with_bind_default(DEFAULT_BIND_PORT)
        .with_identity(wt_identity)
        .keep_alive_interval(Some(std::time::Duration::from_secs(3)))
        .build();

    let server = Endpoint::server(wt_config)?;
    tracing::info!("Gateway started on UDP {} (WebTransport/QUIC)", DEFAULT_BIND_PORT);

    loop {
        let incoming = server.accept().await;
        let input_tx = input_tx.clone();
        let snapshot_rx = snapshot_tx.subscribe();
        tokio::spawn(async move {
            match incoming.await {
                Ok(request) => match request.accept().await {
                    Ok(connection) => {
                        tracing::info!("New WebTransport connection: {:?}", connection.remote_address());
                        handle_webtransport_connection(connection, input_tx, snapshot_rx).await;
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

async fn handle_webtransport_connection(
    connection: wtransport::Connection,
    input_tx: mpsc::Sender<PlayerInput>,
    mut snapshot_rx: broadcast::Receiver<Arc<WorldSnapshot>>,
) {
    // O gateway é a única fonte de verdade sobre entity_id — nunca lemos
    // o campo entity_id enviado pelo cliente.
    let assigned_entity_id = non_zero_entity_id(connection.stable_id() as u32);

    loop {
        tokio::select! {
            received = connection.receive_datagram() => {
                match received {
                    Ok(datagram) => {
                        handle_incoming_datagram(
                            datagram.payload(),
                            assigned_entity_id,
                            &input_tx,
                        ).await;
                    }
                    Err(error) => {
                        tracing::info!("WebTransport connection closed: {}", error);
                        break;
                    }
                }
            }
            snapshot = snapshot_rx.recv() => {
                match snapshot {
                    Ok(snapshot) => {
                        send_snapshot_webtransport(&connection, &snapshot, assigned_entity_id);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("WebTransport receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn handle_incoming_datagram(
    payload: &[u8],
    assigned_entity_id: u32,
    input_tx: &mpsc::Sender<PlayerInput>,
) {
    match PlayerInput::decode(payload) {
        Ok(mut input) => {
            // Sobrescreve qualquer entity_id enviado pelo cliente
            input.entity_id = assigned_entity_id;
            if input_tx.send(input).await.is_err() {
                tracing::warn!("World input channel closed — dropping input");
            }
        }
        Err(error) => {
            tracing::warn!("Invalid PlayerInput datagram: {}", error);
        }
    }
}

fn send_snapshot_webtransport(
    connection: &wtransport::Connection,
    snapshot: &WorldSnapshot,
    assigned_entity_id: u32,
) {
    let mut personalized = snapshot.clone();
    personalized.local_entity_id = assigned_entity_id;
    let mut payload = Vec::with_capacity(personalized.encoded_len());
    if personalized.encode(&mut payload).is_ok() {
        if let Err(error) = connection.send_datagram(&payload) {
            tracing::debug!("Failed to send WebTransport snapshot: {}", error);
        }
    }
}

async fn run_websocket_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<WorldSnapshot>>,
    game_auth: Option<Arc<GameAuthFn>>,
) {
    let bind_addr = format!("0.0.0.0:{}", DEFAULT_WEBSOCKET_PORT);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(error) => {
            tracing::error!("Failed to bind WebSocket gateway on {}: {}", bind_addr, error);
            return;
        }
    };
    tracing::info!("WebSocket fallback gateway started on TCP {}", DEFAULT_WEBSOCKET_PORT);

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
        let game_auth = game_auth.clone();
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(socket) => {
                    tracing::info!("New WebSocket connection: {}", remote_addr);
                    handle_websocket_connection(socket, input_tx, snapshot_rx, game_auth).await;
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
    mut snapshot_rx: broadcast::Receiver<Arc<WorldSnapshot>>,
    game_auth: Option<Arc<GameAuthFn>>,
) {
    let (mut write, mut read) = socket.split();
    let assigned_entity_id = NEXT_WEBSOCKET_ENTITY_ID.fetch_add(1, Ordering::Relaxed);

    // The client sends GameAuthPacket as the very first binary packet if logged
    // in. The handler validates the JWT, loads CharacterData from PG, and pushes
    // a CharacterAssignment to the world. Subsequent packets are always PlayerInput.
    let mut first_packet = true;

    loop {
        tokio::select! {
            received = read.next() => {
                match received {
                    Some(Ok(WsMessage::Binary(payload))) => {
                        if first_packet {
                            first_packet = false;
                            // Try to decode as GameAuthPacket. On success, call the
                            // game_auth handler (which pushes CharacterAssignment to ECS).
                            // On failure or missing handler, fall through to PlayerInput.
                            if let Ok(auth_pkt) = GameAuthPacket::decode(payload.as_ref()) {
                                if let Some(ref handler) = game_auth {
                                    handler(assigned_entity_id, auth_pkt.token).await;
                                }
                                // GameAuthPacket is never forwarded to the world as input.
                                continue;
                            }
                            // Not a GameAuthPacket — anonymous connection; treat as PlayerInput.
                        }
                        handle_incoming_datagram(&payload, assigned_entity_id, &input_tx).await;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::info!("WebSocket error: {}", error);
                        break;
                    }
                }
            }
            snapshot = snapshot_rx.recv() => {
                match snapshot {
                    Ok(snapshot) => {
                        let mut personalized = snapshot.as_ref().clone();
                        personalized.local_entity_id = assigned_entity_id;
                        let mut payload = Vec::with_capacity(personalized.encoded_len());
                        if personalized.encode(&mut payload).is_ok()
                            && write.send(WsMessage::Binary(payload)).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("WebSocket receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn non_zero_entity_id(entity_id: u32) -> u32 {
    if entity_id == 0 { 1 } else { entity_id }
}

// ---------------------------------------------------------------------------
// Auth Gateway — port 8081
// Handles LoginRequest and RegisterRequest over a dedicated WebSocket.
// Isolated from the game input channel so auth messages never reach the world.
// ---------------------------------------------------------------------------

/// Auth channel configuration, injected from the caller to avoid hard deps on
/// database/redis inside the gateway crate.
pub struct AuthHandlers {
    /// Called when a login or register packet arrives.
    /// Returns serialized `LoginResponse` bytes.
    pub handle: Arc<dyn Fn(bool, Vec<u8>) -> futures_util::future::BoxFuture<'static, Vec<u8>> + Send + Sync>,
}

pub async fn run_auth_gateway(handlers: Arc<AuthHandlers>) {
    let bind_addr = format!("0.0.0.0:{}", DEFAULT_AUTH_PORT);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(error) => {
            tracing::error!(%error, "Failed to bind auth gateway on {}", bind_addr);
            return;
        }
    };
    tracing::info!("Auth gateway started on TCP {}", DEFAULT_AUTH_PORT);

    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "Failed to accept auth TCP connection");
                continue;
            }
        };
        let handlers = Arc::clone(&handlers);
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(socket) => {
                    tracing::debug!("Auth WebSocket connection from {}", remote_addr);
                    handle_auth_connection(socket, handlers).await;
                }
                Err(error) => {
                    tracing::warn!(%error, "Failed auth WebSocket handshake from {}", remote_addr);
                }
            }
        });
    }
}

async fn handle_auth_connection(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    handlers: Arc<AuthHandlers>,
) {
    let (mut write, mut read) = socket.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(WsMessage::Binary(payload)) => {
                // Try LoginRequest first; if that fails, try RegisterRequest.
                let (is_register, response_bytes) =
                    if LoginRequest::decode(payload.as_ref()).is_ok() {
                        let resp = (handlers.handle)(false, payload.to_vec()).await;
                        (false, resp)
                    } else if RegisterRequest::decode(payload.as_ref()).is_ok() {
                        let resp = (handlers.handle)(true, payload.to_vec()).await;
                        (true, resp)
                    } else {
                        tracing::warn!("Auth connection: unrecognized payload, closing");
                        break;
                    };

                let _ = is_register;
                if write.send(WsMessage::Binary(response_bytes)).await.is_err() {
                    break;
                }
                // Auth is one-shot per connection: close after responding.
                let _ = write.send(WsMessage::Close(None)).await;
                break;
            }
            Ok(WsMessage::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
}
