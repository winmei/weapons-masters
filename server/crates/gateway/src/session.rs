use std::net::IpAddr;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::mpsc;

use shared::proto::ClientPlatform;

/// TTL do cache local entre consultas Redis. O hot path nunca aguarda I/O;
/// revalidações são disparadas em background quando o cache expira.
pub const SESSION_IP_CACHE_TTL: Duration = Duration::from_secs(30);
/// Janela para o cliente Mobile/Web revalidar após handoff de rede.
pub const REAUTH_CHALLENGE_SECS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientType {
    PC,
    Mobile,
    Web,
}

/// Converte o campo Protobuf declarado pelo cliente. Valores desconhecidos
/// retornam `None` para preservar o default inferido pelo transporte.
pub fn client_type_from_proto(platform: ClientPlatform) -> Option<ClientType> {
    match platform {
        ClientPlatform::Pc => Some(ClientType::PC),
        ClientPlatform::Mobile => Some(ClientType::Mobile),
        ClientPlatform::Web => Some(ClientType::Web),
        ClientPlatform::Unknown => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    Allow,
    ReAuthChallenge { deadline: Instant, new_ip: IpAddr },
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session revoked or expired")]
    SessionRevoked,
    #[error("connection IP does not match session-bound IP")]
    IpMismatch,
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
}

use redis::AsyncCommands;

/// Consulta Redis e aplica a regra de plataforma.
/// Deve ser chamada apenas pelo worker de background — nunca no loop de recepção.
pub async fn validate_session_ip(
    redis: &mut redis::aio::MultiplexedConnection,
    player_id: i64,
    connection_ip: IpAddr,
    client_type: ClientType,
) -> Result<SessionAction, SessionError> {
    let session_key = format!("session:{player_id}:ip");
    let stored_ip: Option<String> = redis.get(&session_key).await?;

    evaluate_stored_ip(stored_ip.as_deref(), connection_ip, client_type)
}

/// Lógica pura separada para testes unitários sem Redis.
pub fn evaluate_stored_ip(
    stored_ip: Option<&str>,
    connection_ip: IpAddr,
    client_type: ClientType,
) -> Result<SessionAction, SessionError> {
    match stored_ip {
        None => Err(SessionError::SessionRevoked),
        Some(ip) if ip.parse::<IpAddr>().ok().as_ref() == Some(&connection_ip) => {
            Ok(SessionAction::Allow)
        }
        Some(stored) => {
            let stored_addr = stored.parse::<IpAddr>().unwrap_or_else(|_| {
                tracing::warn!(stored, "invalid IP stored in Redis session key");
                connection_ip
            });
            if stored_addr == connection_ip {
                return Ok(SessionAction::Allow);
            }

            tracing::warn!(
                %connection_ip,
                %stored_addr,
                ?client_type,
                "connection IP diverges from session-bound IP"
            );

            match client_type {
                ClientType::PC => Err(SessionError::IpMismatch),
                ClientType::Mobile | ClientType::Web => Ok(SessionAction::ReAuthChallenge {
                    deadline: Instant::now() + Duration::from_secs(REAUTH_CHALLENGE_SECS),
                    new_ip: connection_ip,
                }),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketDecision {
    /// Encaminhar pacote ao World Server.
    Allow,
    /// Descartar pacote — sessão inválida ou challenge expirado.
    Reject,
}

#[derive(Debug)]
enum SessionCache {
    Unauthenticated,
    Pending,
    Allowed {
        stored_ip: IpAddr,
        validated_at: Instant,
    },
    ReAuth {
        deadline: Instant,
        _new_ip: IpAddr,
    },
    Denied,
}

#[derive(Debug)]
struct ConnectionSessionState {
    player_id: Option<i64>,
    client_type: ClientType,
    connection_ip: IpAddr,
    cache: SessionCache,
    validation_in_flight: bool,
    require_player_binding: bool,
    outbound_tx: Option<mpsc::Sender<SessionOutbound>>,
}

pub struct ValidationJob {
    player_id: i64,
    connection_ip: IpAddr,
    client_type: ClientType,
    session: Weak<Mutex<ConnectionSessionState>>,
}

/// Evento imediato do Gateway para a conexão (não passa pelo World Server).
#[derive(Debug, Clone)]
pub enum SessionOutbound {
    ReAuthChallenge { deadline_secs: u32 },
    ReAuthResult {
        success: bool,
        access_token: String,
        refresh_token: String,
        error_message: String,
    },
}

/// Estado de sessão por conexão. O loop de recepção só lê o cache local.
#[derive(Clone)]
pub struct ConnectionSession {
    inner: Arc<Mutex<ConnectionSessionState>>,
    validate_tx: Option<mpsc::Sender<ValidationJob>>,
}

impl ConnectionSession {
    pub fn new(
        connection_ip: IpAddr,
        client_type: ClientType,
        validate_tx: Option<mpsc::Sender<ValidationJob>>,
        require_player_binding: bool,
        outbound_tx: Option<mpsc::Sender<SessionOutbound>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConnectionSessionState {
                player_id: None,
                client_type,
                connection_ip,
                cache: SessionCache::Unauthenticated,
                validation_in_flight: false,
                require_player_binding,
                outbound_tx,
            })),
            validate_tx,
        }
    }

    /// Segundos restantes do ReAuthChallenge ativo, para anexar ao snapshot.
    pub fn reauth_challenge_for_snapshot(&self) -> Option<u32> {
        let state = self.inner.lock().unwrap();
        match &state.cache {
            SessionCache::ReAuth { deadline, .. } if Instant::now() <= *deadline => {
                Some(
                    deadline
                        .duration_since(Instant::now())
                        .as_secs()
                        .clamp(1, REAUTH_CHALLENGE_SECS) as u32,
                )
            }
            _ => None,
        }
    }

    pub fn connection_ip(&self) -> IpAddr {
        self.inner.lock().unwrap().connection_ip
    }

    pub fn set_client_type(&self, client_type: ClientType) {
        self.inner.lock().unwrap().client_type = client_type;
    }

    pub fn is_reauth_pending(&self) -> bool {
        matches!(
            self.inner.lock().unwrap().cache,
            SessionCache::ReAuth { .. }
        )
    }

    pub fn is_reauth_for_player(&self, player_id: i64) -> bool {
        let state = self.inner.lock().unwrap();
        state.player_id == Some(player_id)
            && matches!(state.cache, SessionCache::ReAuth { .. })
    }

    /// Chamado após re-bind bem-sucedido no Redis durante network handoff.
    pub fn mark_ip_rebound(&self) {
        let mut state = self.inner.lock().unwrap();
        state.cache = SessionCache::Allowed {
            stored_ip: state.connection_ip,
            validated_at: Instant::now(),
        };
        state.validation_in_flight = false;
    }

    pub fn notify_reauth_result(
        &self,
        success: bool,
        access_token: String,
        refresh_token: String,
        error_message: String,
    ) {
        let Some(tx) = &self.inner.lock().unwrap().outbound_tx else {
            return;
        };
        let _ = tx.try_send(SessionOutbound::ReAuthResult {
            success,
            access_token,
            refresh_token,
            error_message,
        });
    }

    pub fn bind_player(&self, player_id: i64) {
        {
            let mut state = self.inner.lock().unwrap();
            state.player_id = Some(player_id);
            if self.validate_tx.is_none() {
                state.cache = SessionCache::Allowed {
                    stored_ip: state.connection_ip,
                    validated_at: Instant::now(),
                };
                return;
            }
            state.cache = SessionCache::Pending;
        }
        self.enqueue_validation(player_id);
    }

    /// Chamado a cada pacote recebido. Nunca aguarda Redis.
    pub fn check_packet(&self) -> PacketDecision {
        if self.validate_tx.is_none() {
            return PacketDecision::Allow;
        }
        let mut state = self.inner.lock().unwrap();

        if state.require_player_binding && state.player_id.is_none() {
            return PacketDecision::Reject;
        }

        let Some(player_id) = state.player_id else {
            return PacketDecision::Allow;
        };

        match &state.cache {
            SessionCache::Denied => PacketDecision::Reject,
            SessionCache::ReAuth { deadline, .. } if Instant::now() > *deadline => {
                state.cache = SessionCache::Denied;
                tracing::warn!(player_id, "ReAuthChallenge expired — rejecting session");
                PacketDecision::Reject
            }
            SessionCache::ReAuth { .. } => {
                // Challenge ativo: bloqueia gameplay até re-auth atualizar Redis (Step 4).
                Self::maybe_enqueue_validation(&mut state, player_id, &self.validate_tx, &self.inner);
                PacketDecision::Reject
            }
            SessionCache::Allowed {
                stored_ip,
                validated_at,
            } if *stored_ip == state.connection_ip
                && validated_at.elapsed() < SESSION_IP_CACHE_TTL =>
            {
                PacketDecision::Allow
            }
            SessionCache::Allowed { .. } => {
                Self::maybe_enqueue_validation(&mut state, player_id, &self.validate_tx, &self.inner);
                // Cache expirado: fail-closed até revalidação Redis completar.
                PacketDecision::Reject
            }
            SessionCache::Pending | SessionCache::Unauthenticated => {
                if matches!(state.cache, SessionCache::Unauthenticated) {
                    state.cache = SessionCache::Pending;
                }
                Self::maybe_enqueue_validation(&mut state, player_id, &self.validate_tx, &self.inner);
                PacketDecision::Reject
            }
        }
    }

    fn enqueue_validation(&self, player_id: i64) {
        let mut state = self.inner.lock().unwrap();
        Self::maybe_enqueue_validation(&mut state, player_id, &self.validate_tx, &self.inner);
    }

    fn maybe_enqueue_validation(
        state: &mut ConnectionSessionState,
        player_id: i64,
        validate_tx: &Option<mpsc::Sender<ValidationJob>>,
        inner: &Arc<Mutex<ConnectionSessionState>>,
    ) {
        let Some(tx) = validate_tx else {
            return;
        };
        if state.validation_in_flight {
            return;
        }
        state.validation_in_flight = true;

        let job = ValidationJob {
            player_id,
            connection_ip: state.connection_ip,
            client_type: state.client_type,
            session: Arc::downgrade(inner),
        };

        if tx.try_send(job).is_err() {
            state.validation_in_flight = false;
            state.cache = SessionCache::Unauthenticated;
            tracing::warn!(player_id, "SessionValidator queue full — session reset to Unauthenticated");
        }
    }
}

/// Worker de background que executa consultas Redis sem bloquear recepção de pacotes.
pub struct SessionValidator {
    _worker: tokio::task::JoinHandle<()>,
}

impl SessionValidator {
    pub async fn spawn(
        redis_url: &str,
    ) -> Result<(Self, mpsc::Sender<ValidationJob>), redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let (tx, mut rx) = mpsc::channel::<ValidationJob>(256);

        let worker = tokio::spawn(async move {
            let conn = match client.get_multiplexed_tokio_connection().await {
                Ok(c) => c,
                Err(error) => {
                    tracing::error!(?error, "SessionValidator: Redis connection failed");
                    return;
                }
            };

            while let Some(job) = rx.recv().await {
                let mut conn_clone = conn.clone();
                tokio::spawn(async move {
                    let result = validate_session_ip(
                        &mut conn_clone,
                        job.player_id,
                        job.connection_ip,
                        job.client_type,
                    )
                    .await;

                    if let Some(state) = job.session.upgrade() {
                        let mut guard = state.lock().unwrap();
                        guard.validation_in_flight = false;
                        apply_validation_result(&mut guard, job.player_id, result);
                    }
                });
            }
        });

        Ok((Self { _worker: worker }, tx))
    }
}

fn apply_validation_result(
    state: &mut ConnectionSessionState,
    player_id: i64,
    result: Result<SessionAction, SessionError>,
) {
    match result {
        Ok(SessionAction::Allow) => {
            state.cache = SessionCache::Allowed {
                stored_ip: state.connection_ip,
                validated_at: Instant::now(),
            };
        }
        Ok(SessionAction::ReAuthChallenge { deadline, new_ip }) => {
            tracing::info!(
                player_id,
                %new_ip,
                ?deadline,
                "ReAuthChallenge issued for mobile/web network handoff"
            );
            state.cache = SessionCache::ReAuth { deadline, _new_ip: new_ip };
            if let Some(tx) = &state.outbound_tx {
                let deadline_secs = deadline
                    .duration_since(Instant::now())
                    .as_secs()
                    .clamp(1, REAUTH_CHALLENGE_SECS) as u32;
                if tx
                    .try_send(SessionOutbound::ReAuthChallenge { deadline_secs })
                    .is_err()
                {
                    tracing::warn!(player_id, "session outbound queue full — ReAuth notify deferred");
                }
            }
        }
        Err(SessionError::IpMismatch) => {
            tracing::warn!(player_id, "PC session rejected — IP mismatch");
            state.cache = SessionCache::Denied;
        }
        Err(SessionError::SessionRevoked) => {
            tracing::warn!(player_id, "session revoked or expired in Redis");
            state.cache = SessionCache::Denied;
        }
        Err(SessionError::Redis(error)) => {
            tracing::error!(?error, player_id, "Redis validation failed");
            state.cache = SessionCache::Denied;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn matching_ip_allows() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let action = evaluate_stored_ip(Some("192.168.1.10"), ip, ClientType::PC).unwrap();
        assert_eq!(action, SessionAction::Allow);
    }

    #[test]
    fn missing_session_is_revoked() {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let err = evaluate_stored_ip(None, ip, ClientType::PC).unwrap_err();
        assert!(matches!(err, SessionError::SessionRevoked));
    }

    #[test]
    fn pc_ip_mismatch_rejects() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let err = evaluate_stored_ip(Some("192.168.1.10"), ip, ClientType::PC).unwrap_err();
        assert!(matches!(err, SessionError::IpMismatch));
    }

    #[test]
    fn mobile_ip_mismatch_issues_reauth() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let action = evaluate_stored_ip(Some("192.168.1.10"), ip, ClientType::Mobile).unwrap();
        assert!(matches!(action, SessionAction::ReAuthChallenge { .. }));
    }

    #[test]
    fn ipv4_mapped_ipv6_matches_v4() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let action =
            evaluate_stored_ip(Some("192.168.1.10"), ip, ClientType::PC).unwrap();
        assert_eq!(action, SessionAction::Allow);
    }

    #[test]
    fn proto_platform_mapping() {
        use shared::proto::ClientPlatform;
        assert_eq!(client_type_from_proto(ClientPlatform::Pc), Some(ClientType::PC));
        assert_eq!(client_type_from_proto(ClientPlatform::Mobile), Some(ClientType::Mobile));
        assert_eq!(client_type_from_proto(ClientPlatform::Web), Some(ClientType::Web));
        assert_eq!(client_type_from_proto(ClientPlatform::Unknown), None);
    }

    #[test]
    fn web_ip_mismatch_issues_reauth() {
        let ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let action = evaluate_stored_ip(Some("127.0.0.1"), ip, ClientType::Web).unwrap();
        assert!(matches!(action, SessionAction::ReAuthChallenge { .. }));
    }
}
