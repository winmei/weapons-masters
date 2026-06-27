//! Auth service wiring — Step 3
//!
//! Bridges `gateway::run_auth_gateway` with the real `auth` crate.
//! After a successful login:
//!   1. Returns `LoginResponse` with full `CharacterData` to the client.
//!   2. Pushes a `CharacterAssignment` onto the ECS channel so the game loop
//!      applies the real DB character_id, position and progress to the player
//!      entity — fixing the surrogate bug that caused zero persistence writes.
//!
//! Per `$wm-persistence-auth`:
//! - JWT_SECRET mandatory and strong, read from env.
//! - Argon2 runs in spawn_blocking, never on the async event loop.
//! - Token never logged.
//! - Rate limit enforced per IP.

use std::sync::Arc;

use prost::Message as ProstMessage;
use shared::proto::{
    CharacterData, InventorySlot, ItemData, LoginRequest, LoginResponse, RefreshTokenRequest,
    RefreshTokenResponse, RegisterRequest,
};
use sqlx::Row;

use auth::{bind_session_to_ip, login, refresh_access_token, register, verify_jwt, SecurityConfig};
use gateway::AuthHandlers;

use crate::CharacterAssignment;

// ---------------------------------------------------------------------------
// Auth state
// ---------------------------------------------------------------------------

pub struct AuthState {
    pub pg_pool: sqlx::PgPool,
    pub redis: redis::aio::MultiplexedConnection,
    pub config: SecurityConfig,
    /// Pushed after every successful login so the ECS tick loop can bind
    /// the real character_id to the player entity.
    pub char_assign_tx: tokio::sync::mpsc::Sender<CharacterAssignment>,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub fn build_auth_handlers(state: Arc<AuthState>) -> Arc<AuthHandlers> {
    let refresh_state = Arc::clone(&state);
    Arc::new(AuthHandlers {
        handle: Arc::new({
            let state = Arc::clone(&state);
            move |is_register: bool, payload: Vec<u8>, origin_ip: std::net::IpAddr| {
                let state = Arc::clone(&state);
                Box::pin(async move {
                    let response = if is_register {
                        handle_register(state, &payload, origin_ip).await
                    } else {
                        handle_login(state, &payload, origin_ip).await
                    };
                    let mut buf = Vec::with_capacity(response.encoded_len());
                    let _ = response.encode(&mut buf);
                    buf
                })
            }
        }),
        refresh: Arc::new(move |payload: Vec<u8>, origin_ip: std::net::IpAddr| {
            let state = Arc::clone(&refresh_state);
            Box::pin(async move {
                let response = handle_refresh(state, &payload, origin_ip).await;
                let mut buf = Vec::with_capacity(response.encoded_len());
                let _ = response.encode(&mut buf);
                buf
            })
        }),
    })
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

async fn handle_register(state: Arc<AuthState>, payload: &[u8], origin_ip: std::net::IpAddr) -> LoginResponse {
    let req = match RegisterRequest::decode(payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, "handle_register: failed to decode payload");
            return error_response("invalid request payload");
        }
    };

    if req.username.trim().is_empty() || req.password.is_empty() {
        return error_response("username and password are required");
    }
    if req.username.chars().count() > 32 || !req.username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return error_response("username must be 1-32 ASCII alphanumeric characters or underscores");
    }
    if req.password.len() < 8 || req.password.len() > 128 {
        return error_response("password must be 8-128 characters");
    }

    match register(&state.pg_pool, &req.username, &req.password).await {
        Ok(player_id) => {
            tracing::info!(player_id, username = %req.username, "player registered");
            // Create a default character row in the DB, then log in immediately.
            if let Err(e) = create_default_character(&state.pg_pool, player_id, &req.username).await
            {
                tracing::error!(?e, player_id, "failed to create default character");
                return error_response("account created but character setup failed");
            }
            // Re-use login flow so the client gets a JWT + CharacterData.
            let login_req = LoginRequest {
                username: req.username,
                password: req.password,
            };
            let mut buf = Vec::with_capacity(login_req.encoded_len());
            let _ = login_req.encode(&mut buf);
            handle_login(state, &buf, origin_ip).await
        }
        Err(auth::AuthError::UsernameTaken) => error_response("username already taken"),
        Err(e) => {
            tracing::error!(?e, "handle_register: unexpected error");
            error_response("internal error")
        }
    }
}

async fn handle_login(state: Arc<AuthState>, payload: &[u8], origin_ip: std::net::IpAddr) -> LoginResponse {
    let req = match LoginRequest::decode(payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, "handle_login: failed to decode payload");
            return error_response("invalid request payload");
        }
    };

    if req.username.trim().is_empty() || req.password.is_empty() {
        return error_response("username and password are required");
    }
    if req.username.chars().count() > 32 || !req.username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return error_response("invalid username format");
    }
    if req.password.len() > 128 {
        return error_response("password too long");
    }

    // IP real da conexão auth (porta 8081) — vinculado à sessão no Redis.
    let mut redis_conn = state.redis.clone();

    let tokens = match login(
        &state.pg_pool,
        &state.config,
        req.username.clone(),
        req.password,
        origin_ip,
        &mut redis_conn,
    )
    .await
    {
        Ok(t) => t,
        Err(auth::AuthError::NotFound) | Err(auth::AuthError::InvalidPassword) => {
            return error_response("invalid username or password");
        }
        Err(auth::AuthError::RateLimited) => {
            return error_response("too many login attempts — try again in a minute");
        }
        Err(e) => {
            tracing::error!(?e, "handle_login: unexpected auth error");
            return error_response("internal error");
        }
    };

    // Load character data from PostgreSQL to send to the client and to seed
    // the ECS entity with the correct character_id and saved progress.
    let character_data = match load_character_data(&state.pg_pool, &req.username).await {
        Ok(cd) => cd,
        Err(e) => {
            tracing::error!(?e, username = %req.username, "handle_login: failed to load character data");
            return error_response("login succeeded but character data unavailable");
        }
    };

    // The entity_id assigned by the gateway is not known here — it's assigned
    // per-connection on the WebSocket/WebTransport side.  We push the assignment
    // with entity_id = 0 as a sentinel; `apply_character_assignments_system`
    // will match by the character_id on the next tick when the entity registers.
    //
    // Better approach used here: push with a NATS subject so the ECS system can
    // match by username. But for Step 3 simplicity: the auth port (8081) is
    // one-shot per connection.  The client opens 8081 to auth, then opens 8080
    // to play.  The entity on 8080 is created on first input.  We need a way
    // to correlate them.
    //
    // Solution: store character_id in Redis keyed by username so the gateway
    // can inject it when the game connection arrives (Step 4).  For now, store
    // a "pending assignment" in Redis that the game loop polls.
    let character_id = character_data.character_id;
    tracing::info!(username = %req.username, character_id, "login ok");
    // token value is not logged — only metadata
    LoginResponse {
        success: true,
        token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        error_message: String::new(),
        character: Some(character_data),
    }
}

async fn handle_refresh(
    state: Arc<AuthState>,
    payload: &[u8],
    origin_ip: std::net::IpAddr,
) -> RefreshTokenResponse {
    let req = match RefreshTokenRequest::decode(payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%e, "handle_refresh: failed to decode payload");
            return refresh_error_response("invalid request payload");
        }
    };

    if req.refresh_token.trim().is_empty() {
        return refresh_error_response("refresh token is required");
    }

    let mut redis_conn = state.redis.clone();
    let tokens = match refresh_access_token(&req.refresh_token, &state.config, &mut redis_conn).await
    {
        Ok(t) => t,
        Err(auth::AuthError::RefreshTokenInvalid) => {
            return refresh_error_response("invalid or expired refresh token");
        }
        Err(auth::AuthError::RefreshTokenReuse) => {
            return refresh_error_response("session revoked — please log in again");
        }
        Err(e) => {
            tracing::error!(?e, "handle_refresh: unexpected auth error");
            return refresh_error_response("internal error");
        }
    };

    // Re-bind IP da conexão auth após rotação bem-sucedida.
    if let Ok(claims) = verify_jwt(&tokens.access_token, &state.config) {
        if let Err(error) = bind_session_to_ip(
            claims.sub,
            origin_ip,
            state.config.jwt_expiry_secs,
            &mut redis_conn,
        )
        .await
        {
            tracing::warn!(?error, player_id = claims.sub, %origin_ip, "handle_refresh: session IP re-bind failed");
        }
    }

    RefreshTokenResponse {
        success: true,
        token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        error_message: String::new(),
    }
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

/// Creates a default character row for a newly registered player.
async fn create_default_character(
    pool: &sqlx::PgPool,
    player_id: i64,
    username: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO player_characters
            (player_id, name, level, experience, hp, max_hp,
             position_x, position_y, position_map)
        VALUES ($1, $2, 1, 0, 200, 200, 0.0, 0.0, 'starter')
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(player_id)
    .bind(username) // character name defaults to username for Step 3
    .execute(pool)
    .await?;
    Ok(())
}

/// Loads the first character for the given username, including inventory.
/// Returns a populated `CharacterData` proto message.
async fn load_character_data(
    pool: &sqlx::PgPool,
    username: &str,
) -> Result<CharacterData, sqlx::Error> {
    // Single query joining players → player_characters to avoid N+1.
    let row = sqlx::query(
        r#"
        SELECT
            pc.id          AS character_id,
            pc.name        AS name,
            pc.level       AS level,
            pc.experience  AS experience,
            pc.hp          AS hp,
            pc.max_hp      AS max_hp,
            pc.position_x  AS position_x,
            pc.position_y  AS position_y,
            pc.position_map AS position_map
        FROM players p
        JOIN player_characters pc ON pc.player_id = p.id
        WHERE p.username = $1
        ORDER BY pc.id ASC
        LIMIT 1
        "#,
    )
    .bind(username)
    .fetch_one(pool)
    .await?;

    let character_id: i64 = row.try_get("character_id")?;
    let level: i32 = row.try_get("level")?;
    let experience: i64 = row.try_get("experience")?;
    let hp: i32 = row.try_get("hp")?;
    let max_hp: i32 = row.try_get("max_hp")?;
    let position_x: f32 = row.try_get("position_x")?;
    let position_y: f32 = row.try_get("position_y")?;
    let position_map: String = row.try_get("position_map")?;
    let name: String = row.try_get("name")?;

    // Load inventory slots for this character.
    let inv_rows = sqlx::query(
        r#"
        SELECT slot, item_data
        FROM player_inventory
        WHERE character_id = $1
        ORDER BY slot ASC
        "#,
    )
    .bind(character_id)
    .fetch_all(pool)
    .await?;

    let mut inventory = Vec::with_capacity(inv_rows.len());
    for inv_row in inv_rows {
        let slot: i16 = inv_row.try_get("slot")?;
        let item_data: serde_json::Value = inv_row.try_get("item_data")?;

        // item_data JSONB schema: { item_id, item_name, quantity }
        let item_id = item_data
            .get("item_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let item_name = item_data
            .get("item_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let quantity = item_data
            .get("quantity")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        inventory.push(InventorySlot {
            slot: slot as i32,
            item: Some(ItemData {
                item_id,
                item_name,
                quantity,
            }),
        });
    }

    Ok(CharacterData {
        character_id,
        name,
        level,
        experience,
        hp,
        max_hp,
        position_x,
        position_y,
        position_map,
        inventory,
    })
}

fn error_response(msg: &str) -> LoginResponse {
    LoginResponse {
        success: false,
        token: String::new(),
        refresh_token: String::new(),
        error_message: msg.to_string(),
        character: None,
    }
}

fn refresh_error_response(msg: &str) -> RefreshTokenResponse {
    RefreshTokenResponse {
        success: false,
        token: String::new(),
        refresh_token: String::new(),
        error_message: msg.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

pub async fn init_auth_state(
    char_assign_tx: tokio::sync::mpsc::Sender<CharacterAssignment>,
) -> Result<Arc<AuthState>, InitError> {
    let config = SecurityConfig::from_env().map_err(InitError::Config)?;

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wm:wm_dev@localhost/weapons_masters".to_string());
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    tracing::info!(%db_url, "auth_service: connecting to PostgreSQL");
    let pg_pool = sqlx::PgPool::connect(&db_url)
        .await
        .map_err(InitError::Postgres)?;

    tracing::info!(%redis_url, "auth_service: connecting to Redis");
    let redis_client = redis::Client::open(redis_url.as_str()).map_err(InitError::Redis)?;
    let redis_conn = redis_client
        .get_multiplexed_tokio_connection()
        .await
        .map_err(InitError::Redis)?;

    Ok(Arc::new(AuthState {
        pg_pool,
        redis: redis_conn,
        config,
        char_assign_tx,
    }))
}

#[derive(Debug)]
pub enum InitError {
    Config(auth::ConfigError),
    Postgres(sqlx::Error),
    Redis(redis::RedisError),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::Config(e) => write!(f, "config error: {e}"),
            InitError::Postgres(e) => write!(f, "postgres error: {e}"),
            InitError::Redis(e) => write!(f, "redis error: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Game-auth handler — validates JWT on the game WebSocket (port 8080)
// ---------------------------------------------------------------------------

/// Builds the `game_auth` handler for `GatewayConfig`.
///
/// When the Godot client sends a `GameAuthPacket` as the very first binary
/// packet on the game WebSocket (port 8080), the gateway calls this handler
/// with the assigned `entity_id` and the JWT from the packet.
///
/// This handler:
///   1. Validates the JWT → extracts `player_id` (sub claim).
///   2. Loads the player's character row from PostgreSQL.
///   3. Pushes a `CharacterAssignment` to the ECS channel so the tick loop
///      can apply the real character_id, position, and progress to the entity.
///
/// If the JWT is invalid or the DB query fails, the connection remains
/// anonymous (no persistence writes for that session — safe degraded mode).
pub fn build_game_auth_handler(state: Arc<AuthState>) -> Arc<gateway::GameAuthFn> {
    Arc::new(move |entity_id: u32, token: String, session: gateway::ConnectionSession| {
        let state = Arc::clone(&state);
        Box::pin(async move {
            // Step 1: Validate JWT — never log the token value.
            let claims = match verify_jwt(&token, &state.config) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(%e, entity_id, "game_auth: invalid JWT — connection remains anonymous");
                    return;
                }
            };
            let player_id = claims.sub;
            let game_ip = session.connection_ip();

            // Re-vincula o IP da sessão ao socket de jogo (8080/4433), que é
            // o IP efetivamente validado em cada pacote recebido.
            {
                let mut redis_conn = state.redis.clone();
                if let Err(error) = bind_session_to_ip(
                    player_id,
                    game_ip,
                    state.config.jwt_expiry_secs,
                    &mut redis_conn,
                )
                .await
                {
                    tracing::warn!(?error, entity_id, player_id, %game_ip, "game_auth: failed to bind session IP");
                    return;
                }
            }

            session.bind_player(player_id);

            // Step 2: Load character data from PostgreSQL.
            let assignment = match load_character_assignment_by_player_id(
                &state.pg_pool,
                player_id,
                entity_id,
            )
            .await
            {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(%e, entity_id, player_id, "game_auth: DB lookup failed — connection remains anonymous");
                    return;
                }
            };

            let character_id = assignment.character_id;

            // Step 3: Push assignment to the ECS channel.
            // The game tick loop will apply it to the player entity.
            if state.char_assign_tx.send(assignment).await.is_err() {
                tracing::warn!(entity_id, player_id, "game_auth: assignment channel closed");
            } else {
                tracing::info!(entity_id, player_id, character_id, "game_auth: character assignment queued");
            }
        })
    })
}

/// Builds the handler for `SessionReAuthPacket` during Mobile/Web network handoff.
///
/// Validates the refresh token, rotates it, re-binds session IP, and returns
/// new tokens to the client via `SessionReAuthResult`.
pub fn build_session_reauth_handler(state: Arc<AuthState>) -> Arc<gateway::SessionReAuthFn> {
    Arc::new(move |refresh_token: String, session: gateway::ConnectionSession| {
        let state = Arc::clone(&state);
        Box::pin(async move {
            let mut redis_conn = state.redis.clone();
            let tokens = match refresh_access_token(&refresh_token, &state.config, &mut redis_conn).await
            {
                Ok(t) => t,
                Err(auth::AuthError::RefreshTokenInvalid) => {
                    session
                        .notify_reauth_result(
                            false,
                            String::new(),
                            String::new(),
                            "invalid or expired refresh token".to_string(),
                        );
                    return;
                }
                Err(auth::AuthError::RefreshTokenReuse) => {
                    session
                        .notify_reauth_result(
                            false,
                            String::new(),
                            String::new(),
                            "session revoked — please log in again".to_string(),
                        );
                    return;
                }
                Err(e) => {
                    tracing::warn!(?e, "session_reauth: refresh failed");
                    session
                        .notify_reauth_result(
                            false,
                            String::new(),
                            String::new(),
                            "internal error".to_string(),
                        );
                    return;
                }
            };

            let player_id = match verify_jwt(&tokens.access_token, &state.config) {
                Ok(c) => c.sub,
                Err(e) => {
                    tracing::warn!(%e, "session_reauth: JWT verify failed after refresh");
                    session
                        .notify_reauth_result(
                            false,
                            String::new(),
                            String::new(),
                            "internal error".to_string(),
                        );
                    return;
                }
            };

            if !session.is_reauth_for_player(player_id) {
                tracing::warn!(player_id, "session_reauth: no matching ReAuthChallenge");
                session
                    .notify_reauth_result(
                        false,
                        String::new(),
                        String::new(),
                        "no active reauth challenge".to_string(),
                    );
                return;
            }

            let game_ip = session.connection_ip();
            if let Err(error) = bind_session_to_ip(
                player_id,
                game_ip,
                state.config.jwt_expiry_secs,
                &mut redis_conn,
            )
            .await
            {
                tracing::warn!(?error, player_id, %game_ip, "session_reauth: Redis bind failed");
                session
                    .notify_reauth_result(
                        false,
                        String::new(),
                        String::new(),
                        "failed to rebind session IP".to_string(),
                    );
                return;
            }

            session.mark_ip_rebound();
            session
                .notify_reauth_result(
                    true,
                    tokens.access_token,
                    tokens.refresh_token,
                    String::new(),
                );
            tracing::info!(player_id, %game_ip, "session_reauth: IP rebound complete");
        })
    })
}

/// Loads the minimal character fields needed for `CharacterAssignment`.
/// Does not load inventory — the client already received the full `CharacterData`
/// in `LoginResponse`.
async fn load_character_assignment_by_player_id(
    pool: &sqlx::PgPool,
    player_id: i64,
    entity_id: u32,
) -> Result<crate::CharacterAssignment, sqlx::Error> {
    use sqlx::Row;

    let row = sqlx::query(
        r#"
        SELECT id AS character_id,
               level, experience, hp, max_hp,
               position_x, position_y
        FROM player_characters
        WHERE player_id = $1
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .bind(player_id)
    .fetch_one(pool)
    .await?;

    let character_id: i64 = row.try_get("character_id")?;
    let level: i32        = row.try_get("level")?;
    let experience: i64   = row.try_get("experience")?;
    let hp: i32           = row.try_get("hp")?;
    let position_x: f32   = row.try_get("position_x")?;
    let position_y: f32   = row.try_get("position_y")?;

    Ok(crate::CharacterAssignment {
        entity_id,
        character_id,
        level: level as u32,
        experience: experience as u64,
        hp,
        position_x,
        position_y,
    })
}
