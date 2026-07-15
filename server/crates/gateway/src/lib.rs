use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use session::ClientType;
use shared::proto::{
    GameAuthPacket, PlayerInput, SessionReAuthChallenge, SessionReAuthPacket,
    SessionReAuthResult, WorldSnapshot,
};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use wtransport::tls::Sha256DigestFmt;
use wtransport::{Endpoint, Identity, ServerConfig};

mod session;

pub use session::{
    client_type_from_proto, evaluate_stored_ip, validate_session_ip,
    ClientType as SessionClientType, ConnectionSession, PacketDecision, SessionAction,
    SessionError, SessionOutbound, SessionValidator, ValidationJob, REAUTH_CHALLENGE_SECS,
    SESSION_IP_CACHE_TTL,
};

pub const DEFAULT_BIND_PORT: u16 = 4433;
pub const DEFAULT_WEBSOCKET_PORT: u16 = 8080;
/// Auth requests arrive on a dedicated port so they never reach the game input channel.
pub const DEFAULT_AUTH_PORT: u16 = 8081;

// IDs WebSocket começam em 1_000_000 para não colidir com IDs WebTransport
// baseados em connection.stable_id(), que são tipicamente pequenos.
static NEXT_WEBSOCKET_ENTITY_ID: AtomicU32 = AtomicU32::new(1_000_000);

/// Called by the game WebSocket handler when a `GameAuthPacket` is received.
/// Receives the assigned entity_id, the JWT token, and the per-connection session
/// guard so the handler can bind `player_id` for IP validation on every packet.
/// `None` return means the game_auth handler is not configured (dev mode).
pub type GameAuthFn = dyn Fn(u32, String, ConnectionSession) -> futures_util::future::BoxFuture<'static, ()>
    + Send
    + Sync;

/// Called when a `SessionReAuthPacket` arrives during an active ReAuthChallenge.
pub type SessionReAuthFn = dyn Fn(String, ConnectionSession) -> futures_util::future::BoxFuture<'static, ()>
    + Send
    + Sync;

/// Optional game-auth handler injected at startup.
/// When `None`, `GameAuthPacket` is silently ignored (dev/test mode).
pub struct GatewayConfig {
    pub game_auth: Option<Arc<GameAuthFn>>,
    pub session_reauth: Option<Arc<SessionReAuthFn>>,
    /// Channel to the background SessionValidator worker. When `None`, IP
    /// validation is disabled (dev/test mode without Redis).
    pub session_validate_tx: Option<mpsc::Sender<ValidationJob>>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            game_auth: None,
            session_reauth: None,
            session_validate_tx: None,
        }
    }
}

pub async fn run_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<Vec<u8>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_gateway_with_config(input_tx, snapshot_tx, GatewayConfig::default()).await
}

pub async fn run_gateway_with_config(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<Vec<u8>>>,
    config: GatewayConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_validate_tx = config.session_validate_tx.clone();
    let require_player_binding = config.game_auth.is_some();
    let session_reauth = config.session_reauth.clone();
    tokio::spawn(run_websocket_gateway(
        input_tx.clone(),
        snapshot_tx.clone(),
        config.game_auth.clone(),
        session_reauth,
        session_validate_tx.clone(),
        require_player_binding,
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
        let game_auth = config.game_auth.clone();
        let session_reauth = config.session_reauth.clone();
        let session_validate_tx = config.session_validate_tx.clone();
        let require_player_binding = config.game_auth.is_some();
        tokio::spawn(async move {
            match incoming.await {
                Ok(request) => match request.accept().await {
                    Ok(connection) => {
                        tracing::info!("New WebTransport connection: {:?}", connection.remote_address());
                        handle_webtransport_connection(
                            connection,
                            input_tx,
                            snapshot_rx,
                            game_auth,
                            session_reauth,
                            session_validate_tx,
                            require_player_binding,
                        ).await;
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
    mut snapshot_rx: broadcast::Receiver<Arc<Vec<u8>>>,
    game_auth: Option<Arc<GameAuthFn>>,
    session_reauth: Option<Arc<SessionReAuthFn>>,
    session_validate_tx: Option<mpsc::Sender<ValidationJob>>,
    require_player_binding: bool,
) {
    let connection_ip = connection.remote_address().ip();
    let (session_out_tx, mut session_out_rx) = mpsc::channel::<SessionOutbound>(8);
    let session = ConnectionSession::new(
        connection_ip,
        ClientType::Web,
        session_validate_tx,
        require_player_binding,
        Some(session_out_tx),
    );

    let assigned_entity_id = non_zero_entity_id(connection.stable_id() as u32);
    let mut first_packet = true;
    
    let mut packet_count = 0;
    let mut last_reset = std::time::Instant::now();
    const MAX_PACKETS_PER_SEC: u32 = 60;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)), if first_packet => {
                tracing::warn!(assigned_entity_id, "WebTransport connection timeout waiting for first packet");
                break;
            }
            received = connection.receive_datagram() => {
                match received {
                    Ok(datagram) => {
                        let payload = datagram.payload();
                        if first_packet {
                            first_packet = false;
                            match try_handle_first_auth_packet(
                                &payload,
                                assigned_entity_id,
                                &game_auth,
                                &session,
                                require_player_binding,
                            ).await {
                                FirstAuthOutcome::Handled => continue,
                                FirstAuthOutcome::Rejected => break,
                                FirstAuthOutcome::NotAuth => {}
                            }
                        }
                        
                        // [Rate Limiting] Evita DoS por spam de pacotes UDP/QUIC
                        if last_reset.elapsed() >= std::time::Duration::from_secs(1) {
                            packet_count = 0;
                            last_reset = std::time::Instant::now();
                        }
                        packet_count += 1;
                        if packet_count > MAX_PACKETS_PER_SEC {
                            if packet_count == MAX_PACKETS_PER_SEC + 1 {
                                tracing::warn!(assigned_entity_id, "Rate limit exceeded — dropping incoming datagrams");
                            }
                            continue;
                        }

                        handle_incoming_datagram(
                            &payload,
                            assigned_entity_id,
                            &input_tx,
                            &session,
                            &session_reauth,
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
                    Ok(base_payload) => {
                        let personalized =
                            personalize_snapshot(&base_payload, assigned_entity_id, &session).await;
                        send_snapshot_webtransport(&connection, &personalized);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("WebTransport receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            outbound = session_out_rx.recv() => {
                match outbound {
                    Some(SessionOutbound::ReAuthChallenge { deadline_secs }) => {
                        let challenge_payload = build_reauth_challenge_snapshot(assigned_entity_id, deadline_secs);
                        send_snapshot_webtransport(&connection, &challenge_payload);
                    }
                    Some(SessionOutbound::ReAuthResult {
                        success,
                        access_token,
                        refresh_token,
                        error_message,
                    }) => {
                        let result_payload = build_reauth_result_snapshot(
                            assigned_entity_id,
                            success,
                            access_token,
                            refresh_token,
                            error_message,
                        );
                        send_snapshot_webtransport(&connection, &result_payload);
                    }
                    None => break,
                }
            }
        }
    }
}

enum FirstAuthOutcome {
    Handled,
    Rejected,
    NotAuth,
}

/// Max size for the first authentication packet (GameAuthPacket).
/// A legitimate packet contains a JWT (~512 bytes) + enum field (~10 bytes).
/// Limit prevents memory DoS via oversized payloads before protobuf decode.
const MAX_FIRST_PACKET_BYTES: usize = 1024;

async fn try_handle_first_auth_packet(
    payload: &[u8],
    assigned_entity_id: u32,
    game_auth: &Option<Arc<GameAuthFn>>,
    session: &ConnectionSession,
    require_player_binding: bool,
) -> FirstAuthOutcome {
    if payload.len() > MAX_FIRST_PACKET_BYTES {
        tracing::warn!(
            assigned_entity_id,
            payload_len = payload.len(),
            "game connection rejected: first packet exceeds 1KB limit"
        );
        return FirstAuthOutcome::Rejected;
    }

    let Ok(auth_pkt) = GameAuthPacket::decode(payload) else {
        if require_player_binding {
            tracing::warn!(
                assigned_entity_id,
                "game connection rejected: GameAuthPacket required as first packet"
            );
            return FirstAuthOutcome::Rejected;
        }
        return FirstAuthOutcome::NotAuth;
    };

    if let Some(client_type) = client_type_from_proto(
        shared::proto::ClientPlatform::try_from(auth_pkt.client_platform).ok()
            .unwrap_or(shared::proto::ClientPlatform::Unknown),
    ) {
        session.set_client_type(client_type);
    }

    if let Some(ref handler) = game_auth {
        handler(assigned_entity_id, auth_pkt.token, session.clone()).await;
    }
    FirstAuthOutcome::Handled
}

async fn try_handle_session_reauth(
    payload: &[u8],
    session: &ConnectionSession,
    session_reauth: &Option<Arc<SessionReAuthFn>>,
) -> bool {
    // Protobuf decoding is permissive: a regular PlayerInput can decode as an
    // empty SessionReAuthPacket. Only try that packet shape while a challenge
    // is actually active, otherwise normal gameplay input gets swallowed.
    if !session.is_reauth_pending() {
        return false;
    }

    let Ok(reauth_pkt) = SessionReAuthPacket::decode(payload) else {
        return false;
    };

    let refresh_token = if reauth_pkt.refresh_token.is_empty() {
        tracing::warn!("SessionReAuthPacket missing refresh_token");
        session
            .notify_reauth_result(
                false,
                String::new(),
                String::new(),
                "refresh token required".to_string(),
            );
        return true;
    } else {
        reauth_pkt.refresh_token
    };

    if let Some(ref handler) = session_reauth {
        handler(refresh_token, session.clone()).await;
    }
    true
}

#[cfg(test)]
mod packet_routing_tests {
    use super::*;

    #[tokio::test]
    async fn player_input_is_not_consumed_as_reauth_without_active_challenge() {
        let session = ConnectionSession::new(
            "127.0.0.1".parse().unwrap(),
            ClientType::PC,
            None,
            false,
            None,
        );
        let input = PlayerInput {
            entity_id: 7,
            sequence: 42,
            ..Default::default()
        };

        assert!(
            !try_handle_session_reauth(&input.encode_to_vec(), &session, &None).await,
            "regular gameplay input must continue to PlayerInput decoding"
        );
    }
}

async fn handle_incoming_datagram(
    payload: &[u8],
    assigned_entity_id: u32,
    input_tx: &mpsc::Sender<PlayerInput>,
    session: &ConnectionSession,
    session_reauth: &Option<Arc<SessionReAuthFn>>,
) {
    if try_handle_session_reauth(payload, session, session_reauth).await {
        return;
    }

    if session.check_packet() == PacketDecision::Reject {
        tracing::debug!(assigned_entity_id, "dropping packet — session IP validation rejected");
        return;
    }

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

fn build_reauth_challenge_snapshot(entity_id: u32, deadline_secs: u32) -> Vec<u8> {
    let overlay = WorldSnapshot {
        local_entity_id: entity_id,
        session_reauth_challenge: Some(SessionReAuthChallenge { deadline_secs }),
        ..Default::default()
    };
    let mut payload = Vec::with_capacity(overlay.encoded_len());
    let _ = overlay.encode(&mut payload);
    payload
}

fn build_reauth_result_snapshot(
    entity_id: u32,
    success: bool,
    access_token: String,
    refresh_token: String,
    error_message: String,
) -> Vec<u8> {
    let overlay = WorldSnapshot {
        local_entity_id: entity_id,
        session_reauth_result: Some(SessionReAuthResult {
            success,
            access_token,
            refresh_token,
            error_message,
        }),
        ..Default::default()
    };
    let mut payload = Vec::with_capacity(overlay.encoded_len());
    let _ = overlay.encode(&mut payload);
    payload
}

async fn personalize_snapshot(
    base_payload: &[u8],
    assigned_entity_id: u32,
    session: &ConnectionSession,
) -> Vec<u8> {
    let mut overlay = WorldSnapshot::default();
    overlay.local_entity_id = assigned_entity_id;
    if let Some(deadline_secs) = session.reauth_challenge_for_snapshot() {
        overlay.session_reauth_challenge = Some(SessionReAuthChallenge { deadline_secs });
    }
    
    let overlay_len = overlay.encoded_len();
    let mut final_payload = Vec::with_capacity(base_payload.len() + overlay_len);
    final_payload.extend_from_slice(base_payload);
    let _ = overlay.encode(&mut final_payload);
    final_payload
}

fn send_snapshot_webtransport(connection: &wtransport::Connection, payload: &[u8]) {
    if let Err(error) = connection.send_datagram(payload) {
        tracing::debug!("Failed to send WebTransport snapshot: {}", error);
    }
}

async fn run_websocket_gateway(
    input_tx: mpsc::Sender<PlayerInput>,
    snapshot_tx: broadcast::Sender<Arc<Vec<u8>>>,
    game_auth: Option<Arc<GameAuthFn>>,
    session_reauth: Option<Arc<SessionReAuthFn>>,
    session_validate_tx: Option<mpsc::Sender<ValidationJob>>,
    require_player_binding: bool,
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
        let session_reauth = session_reauth.clone();
        let session_validate_tx = session_validate_tx.clone();
        let require_player_binding = require_player_binding;
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(socket) => {
                    tracing::info!("New WebSocket connection: {}", remote_addr);
                    handle_websocket_connection(
                        socket,
                        remote_addr.ip(),
                        input_tx,
                        snapshot_rx,
                        game_auth,
                        session_reauth,
                        session_validate_tx,
                        require_player_binding,
                    ).await;
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
    connection_ip: IpAddr,
    input_tx: mpsc::Sender<PlayerInput>,
    mut snapshot_rx: broadcast::Receiver<Arc<Vec<u8>>>,
    game_auth: Option<Arc<GameAuthFn>>,
    session_reauth: Option<Arc<SessionReAuthFn>>,
    session_validate_tx: Option<mpsc::Sender<ValidationJob>>,
    require_player_binding: bool,
) {
    let (mut write, mut read) = socket.split();
    let assigned_entity_id = NEXT_WEBSOCKET_ENTITY_ID.fetch_add(1, Ordering::Relaxed);
    let (session_out_tx, mut session_out_rx) = mpsc::channel::<SessionOutbound>(8);

    let session = ConnectionSession::new(
        connection_ip,
        ClientType::PC,
        session_validate_tx,
        require_player_binding,
        Some(session_out_tx),
    );

    // The client sends GameAuthPacket as the very first binary packet if logged
    // in. The handler validates the JWT, loads CharacterData from PG, and pushes
    // an authenticated enter-world command to the world. Subsequent packets are PlayerInput.
    let mut first_packet = true;

    loop {
        tokio::select! {
            received = read.next() => {
                match received {
                    Some(Ok(WsMessage::Binary(payload))) => {
                        if first_packet {
                            first_packet = false;
                            match try_handle_first_auth_packet(
                                &payload,
                                assigned_entity_id,
                                &game_auth,
                                &session,
                                require_player_binding,
                            ).await {
                                FirstAuthOutcome::Handled => continue,
                                FirstAuthOutcome::Rejected => break,
                                FirstAuthOutcome::NotAuth => {}
                            }
                        }
                        handle_incoming_datagram(
                            &payload,
                            assigned_entity_id,
                            &input_tx,
                            &session,
                            &session_reauth,
                        ).await;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::error!("WebSocket error: {}", error);
                        break;
                    }
                }
            }
            snapshot = snapshot_rx.recv() => {
                match snapshot {
                    Ok(base_payload) => {
                        let personalized_payload =
                            personalize_snapshot(base_payload.as_ref(), assigned_entity_id, &session).await;
                        if write.send(WsMessage::Binary(personalized_payload)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!("WebSocket receiver skipped {} snapshots", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            outbound = session_out_rx.recv() => {
                match outbound {
                    Some(SessionOutbound::ReAuthChallenge { deadline_secs }) => {
                        let challenge_payload = build_reauth_challenge_snapshot(assigned_entity_id, deadline_secs);
                        if write.send(WsMessage::Binary(challenge_payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(SessionOutbound::ReAuthResult {
                        success,
                        access_token,
                        refresh_token,
                        error_message,
                    }) => {
                        let result_payload = build_reauth_result_snapshot(
                            assigned_entity_id,
                            success,
                            access_token,
                            refresh_token,
                            error_message,
                        );
                        if write.send(WsMessage::Binary(result_payload)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
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
    /// Login ou register. Retorna bytes serializados de `LoginResponse`.
    pub handle: Arc<
        dyn Fn(bool, Vec<u8>, IpAddr) -> futures_util::future::BoxFuture<'static, Vec<u8>>
            + Send
            + Sync,
    >,
    /// Refresh token rotativo. Retorna bytes serializados de `RefreshTokenResponse`.
    pub refresh: Arc<
        dyn Fn(Vec<u8>, IpAddr) -> futures_util::future::BoxFuture<'static, Vec<u8>> + Send + Sync,
    >,
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

        let current = ACTIVE_AUTH_CONNECTIONS.load(Ordering::Relaxed);
        if current >= MAX_CONCURRENT_AUTH_CONNECTIONS {
            tracing::warn!(
                %remote_addr,
                active = current,
                "auth gateway: connection rejected — max concurrent connections exceeded"
            );
            drop(stream);
            continue;
        }

        ACTIVE_AUTH_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
        let handlers = Arc::clone(&handlers);
        tokio::spawn(async move {
            match tokio::time::timeout(std::time::Duration::from_secs(5), tokio_tungstenite::accept_async(stream)).await {
                Ok(Ok(socket)) => {
                    tracing::debug!("Auth WebSocket connection from {}", remote_addr);
                    handle_auth_connection(socket, remote_addr, handlers).await;
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Failed auth WebSocket handshake from {}", remote_addr);
                }
                Err(_) => {
                    tracing::warn!("Auth WebSocket handshake timed out from {}", remote_addr);
                }
            }
            ACTIVE_AUTH_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

/// Auth message type discriminator (first byte of payload on port 8081).
/// Prevents protobuf decode ambiguity between LoginRequest and RegisterRequest
/// which share identical field numbers.
const AUTH_MSG_LOGIN: u8 = 0;
const AUTH_MSG_REGISTER: u8 = 1;
const AUTH_MSG_REFRESH: u8 = 2;

/// Max concurrent connections on the auth gateway (port 8081).
/// Prevents DDoS via connection flooding before reaching Redis rate limit.
const MAX_CONCURRENT_AUTH_CONNECTIONS: usize = 100;
static ACTIVE_AUTH_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

async fn handle_auth_connection(
    socket: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    remote_addr: std::net::SocketAddr,
    handlers: Arc<AuthHandlers>,
) {
    let (mut write, mut read) = socket.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(WsMessage::Binary(payload)) => {
                tracing::info!("Received auth binary payload of {} bytes", payload.len());
                if payload.is_empty() {
                    tracing::warn!("Auth connection: empty payload, closing");
                    break;
                }
                let origin_ip = remote_addr.ip();
                let msg_type = payload[0];
                let proto_data = &payload[1..];
                tracing::info!("Handling auth message type {}", msg_type);

                let response_bytes = match msg_type {
                    AUTH_MSG_LOGIN => (handlers.handle)(false, proto_data.to_vec(), origin_ip).await,
                    AUTH_MSG_REGISTER => (handlers.handle)(true, proto_data.to_vec(), origin_ip).await,
                    AUTH_MSG_REFRESH => (handlers.refresh)(proto_data.to_vec(), origin_ip).await,
                    _ => {
                        tracing::warn!(msg_type, "Auth connection: unknown message type, closing");
                        break;
                    }
                };

                tracing::info!("Sending binary response of {} bytes", response_bytes.len());
                let send_res = write.send(WsMessage::Binary(response_bytes)).await;
                if let Err(e) = send_res {
                    tracing::error!("Auth response send failed: {:?}", e);
                    break;
                }
                tracing::info!("Auth response sent successfully!");
                // Auth is one-shot. We sent the response.
                // DO NOT close the socket here! If we drop the TCP stream immediately,
                // Godot's WebSocket client interprets it as an Abnormal Closure (Code 0)
                // and discards the unread binary payload in its buffer.
                // Instead, we just wait. The client will transition scenes and drop the socket,
                // which will cause read.next().await to yield None and exit cleanly.
            }
            Ok(WsMessage::Close(_)) => break,
            Err(e) => {
                tracing::error!("Auth WebSocket read error: {:?}", e);
                break;
            }
            Ok(_) => {}
        }
    }
}
