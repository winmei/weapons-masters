use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task;
use sqlx::PgPool;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use argon2::password_hash::{PasswordHasher, SaltString, rand_core::{OsRng, RngCore}};
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use serde::{Serialize, Deserialize};
use thiserror::Error;
use redis::AsyncCommands;

const MAX_LOGIN_ATTEMPTS_PER_MINUTE: u64 = 5;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;
/// TTL do refresh token opaco no Redis (7 dias).
pub const REFRESH_TOKEN_TTL_SECS: u64 = 7 * 24 * 3600;
/// Janela para detectar reutilização de refresh token já rotacionado.
const REFRESH_REUSE_WINDOW_SECS: u64 = 24 * 3600;

pub struct SecurityConfig {
    pub jwt_secret: String,
    pub jwt_expiry_secs: u64,
}

impl SecurityConfig {
    /// Constrói a configuração lendo obrigatoriamente de variáveis de ambiente.
    /// Falha em startup se JWT_SECRET não estiver definido — intencional para
    /// impedir que o secret padrão vaze em produção.
    pub fn from_env() -> Result<Self, ConfigError> {
        let jwt_secret = std::env::var("JWT_SECRET").map_err(|_| ConfigError::MissingJwtSecret)?;

        if jwt_secret.len() < 32 {
            return Err(ConfigError::WeakJwtSecret);
        }

        let jwt_expiry_secs = std::env::var("JWT_EXPIRY_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(900);

        Ok(Self { jwt_secret, jwt_expiry_secs })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("JWT_SECRET environment variable is not set")]
    MissingJwtSecret,
    #[error("JWT_SECRET must be at least 32 characters")]
    WeakJwtSecret,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub exp: u64,
    pub iss: String,
}

#[derive(Debug, Clone)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("user not found")]
    NotFound,
    #[error("invalid password")]
    InvalidPassword,
    #[error("username already taken")]
    UsernameTaken,
    #[error("too many login attempts — try again in a minute")]
    RateLimited,
    #[error("internal error")]
    InternalError,
    #[error("jwt error: {0}")]
    JwtError(String),
    #[error("refresh token invalid or expired")]
    RefreshTokenInvalid,
    #[error("refresh token reuse detected — session revoked")]
    RefreshTokenReuse,
}

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| {
            tracing::error!(?error, "hash_password: argon2 failed");
            AuthError::InternalError
        })
}

/// Verifica a senha contra o hash armazenado. Função pura e síncrona — deve
/// ser chamada via `spawn_blocking` para não bloquear o event loop do Tokio.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed_hash = PasswordHash::new(hash).map_err(|error| {
        tracing::error!(?error, "verify_password: failed to parse stored hash");
        AuthError::InternalError
    })?;
    Ok(Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok())
}

const JWT_ISSUER: &str = "weapons-masters";

pub fn create_jwt(player_id: i64, config: &SecurityConfig) -> Result<String, AuthError> {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + config.jwt_expiry_secs;

    let claims = Claims { sub: player_id, exp: expiry, iss: JWT_ISSUER.to_string() };
    let header = Header::default();
    encode(
        &header,
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|error| AuthError::JwtError(error.to_string()))
}

pub fn verify_jwt(token: &str, config: &SecurityConfig) -> Result<Claims, AuthError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[JWT_ISSUER]);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|error| AuthError::JwtError(error.to_string()))
}

pub async fn register(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<i64, AuthError> {
    let password_owned = password.to_string();
    let password_hash = task::spawn_blocking(move || hash_password(&password_owned))
        .await
        .map_err(|e| {
            tracing::error!(?e, "register: spawn_blocking join error");
            AuthError::InternalError
        })??;

    let row = sqlx::query(
        "INSERT INTO players (username, password_hash) VALUES ($1, $2) RETURNING id",
    )
    .bind(username)
    .bind(&password_hash)
    .fetch_one(pool)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            AuthError::UsernameTaken
        } else {
            tracing::error!(?error, %username, "register: DB error");
            AuthError::InternalError
        }
    })?;

    use sqlx::Row;
    let player_id: i64 = row.try_get("id").map_err(|error| {
        tracing::error!(?error, "register: failed to read returned id");
        AuthError::InternalError
    })?;

    Ok(player_id)
}

pub async fn login(
    pool: &PgPool,
    config: &SecurityConfig,
    username: String,
    password: String,
    origin_ip: IpAddr,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<AuthTokens, AuthError> {
    enforce_rate_limit(origin_ip, redis).await?;

    let (player_id, password_hash) = fetch_player_credentials(pool, &username).await?;

    let password_is_valid = task::spawn_blocking(move || verify_password(&password, &password_hash))
        .await
        .map_err(|error| {
            tracing::error!(?error, "login: spawn_blocking join error");
            AuthError::InternalError
        })??;

    if !password_is_valid {
        return Err(AuthError::InvalidPassword);
    }

    let tokens = issue_auth_tokens(player_id, config, redis).await?;
    bind_session_to_ip(player_id, origin_ip, config.jwt_expiry_secs, redis).await?;

    tracing::info!(player_id, %origin_ip, "login successful");
    Ok(tokens)
}

/// Rotaciona o refresh token (one-time use) e emite novo par access + refresh.
pub async fn refresh_access_token(
    refresh_token: &str,
    config: &SecurityConfig,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<AuthTokens, AuthError> {
    if refresh_token.trim().is_empty() {
        return Err(AuthError::RefreshTokenInvalid);
    }

    let player_id = rotate_refresh_token(refresh_token, redis).await?;
    issue_auth_tokens(player_id, config, redis).await
}

async fn enforce_rate_limit(
    origin_ip: IpAddr,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    let rate_key = format!("rate:login:{}", origin_ip);

    let attempts: u64 = redis::cmd("INCR")
        .arg(&rate_key)
        .query_async(redis)
        .await
        .map_err(|error| {
            tracing::error!(?error, "rate limit: Redis INCR failed — blocking login");
            AuthError::InternalError
        })?;

    if attempts == 1 {
        // Define o TTL apenas na primeira tentativa para não renovar a janela a cada request
        let _: () = redis::cmd("EXPIRE")
            .arg(&rate_key)
            .arg(RATE_LIMIT_WINDOW_SECS)
            .query_async(redis)
            .await
            .unwrap_or(());
    }

    if attempts > MAX_LOGIN_ATTEMPTS_PER_MINUTE {
        tracing::warn!(%origin_ip, attempts, "login rate limit exceeded");
        return Err(AuthError::RateLimited);
    }

    Ok(())
}

async fn fetch_player_credentials(
    pool: &PgPool,
    username: &str,
) -> Result<(i64, String), AuthError> {
    use sqlx::Row;
    let row = sqlx::query("SELECT id, password_hash FROM players WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
        .map_err(|error| {
            tracing::error!(?error, %username, "fetch_player_credentials: DB error");
            AuthError::InternalError
        })?
        .ok_or(AuthError::NotFound)?;

    let player_id: i64 = row.try_get("id").map_err(|_| AuthError::InternalError)?;
    let password_hash: String = row.try_get("password_hash").map_err(|_| AuthError::InternalError)?;

    Ok((player_id, password_hash))
}

pub async fn bind_session_to_ip(
    player_id: i64,
    origin_ip: IpAddr,
    expiry_secs: u64,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    let session_key = format!("session:{player_id}:ip");
    let _: () = redis::cmd("SET")
        .arg(&session_key)
        .arg(origin_ip.to_string())
        .arg("EX")
        .arg(expiry_secs)
        .query_async(redis)
        .await
        .map_err(|error| {
            tracing::error!(?error, player_id, "bind_session_to_ip: Redis SET failed");
            AuthError::InternalError
        })?;
    Ok(())
}

fn generate_opaque_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn refresh_token_key(token: &str) -> String {
    format!("refresh:tok:{token}")
}

fn refresh_player_key(player_id: i64) -> String {
    format!("refresh:player:{player_id}")
}

fn refresh_used_key(token: &str) -> String {
    format!("refresh:used:{token}")
}

async fn revoke_player_sessions(
    player_id: i64,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    let player_key = refresh_player_key(player_id);
    let current_token: Option<String> = redis.get(&player_key).await.map_err(|error| {
        tracing::error!(?error, player_id, "revoke_player_sessions: Redis GET failed");
        AuthError::InternalError
    })?;

    if let Some(token) = current_token {
        let _: () = redis::cmd("DEL")
            .arg(refresh_token_key(&token))
            .query_async(redis)
            .await
            .unwrap_or(());
    }

    let session_key = format!("session:{player_id}:ip");
    let _: () = redis::cmd("DEL")
        .arg(&session_key)
        .arg(&player_key)
        .query_async(redis)
        .await
        .unwrap_or(());

    tracing::warn!(player_id, "all refresh/session tokens revoked");
    Ok(())
}

async fn invalidate_player_refresh_token(
    player_id: i64,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    let player_key = refresh_player_key(player_id);
    let current_token: Option<String> = redis.get(&player_key).await.map_err(|error| {
        tracing::error!(?error, player_id, "invalidate_player_refresh_token: Redis GET failed");
        AuthError::InternalError
    })?;

    if let Some(token) = current_token {
        let _: () = redis::cmd("DEL")
            .arg(refresh_token_key(&token))
            .arg(&player_key)
            .query_async(redis)
            .await
            .unwrap_or(());
    }

    Ok(())
}

async fn store_refresh_token(
    player_id: i64,
    refresh_token: &str,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    invalidate_player_refresh_token(player_id, redis).await?;

    let tok_key = refresh_token_key(refresh_token);
    let player_key = refresh_player_key(player_id);

    let _: () = redis::cmd("SET")
        .arg(&tok_key)
        .arg(player_id)
        .arg("EX")
        .arg(REFRESH_TOKEN_TTL_SECS)
        .query_async(redis)
        .await
        .map_err(|error| {
            tracing::error!(?error, player_id, "store_refresh_token: Redis SET failed");
            AuthError::InternalError
        })?;

    let _: () = redis::cmd("SET")
        .arg(&player_key)
        .arg(refresh_token)
        .arg("EX")
        .arg(REFRESH_TOKEN_TTL_SECS)
        .query_async(redis)
        .await
        .map_err(|error| {
            tracing::error!(?error, player_id, "store_refresh_token: Redis SET player failed");
            AuthError::InternalError
        })?;

    Ok(())
}

async fn issue_auth_tokens(
    player_id: i64,
    config: &SecurityConfig,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<AuthTokens, AuthError> {
    let access_token = create_jwt(player_id, config)?;
    let refresh_token = generate_opaque_token();
    store_refresh_token(player_id, &refresh_token, redis).await?;
    Ok(AuthTokens {
        access_token,
        refresh_token,
    })
}

/// Lua script executed atomically in Redis to prevent race conditions on
/// refresh token rotation. The check-delete-mark sequence must be indivisible
/// to avoid two concurrent requests both succeeding and cloning a session.
///
/// Returns:
///   * `player_id` (> 0) — rotation succeeded, token consumed.
///   * `-1` — reuse detected (used_key already existed).
///   * `0` — token not found or already consumed.
const ROTATE_REFRESH_TOKEN_LUA: &str = r#"
local used_key = KEYS[1]
local tok_key  = KEYS[2]
local ttl      = tonumber(ARGV[1])

if redis.call('EXISTS', used_key) == 1 then
    return -1
end

local player_id = redis.call('GET', tok_key)
if not player_id then
    return 0
end

redis.call('DEL', tok_key)
redis.call('SET', used_key, player_id, 'EX', ttl)

return player_id
"#;

async fn rotate_refresh_token(
    presented_token: &str,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<i64, AuthError> {
    let used_key = refresh_used_key(presented_token);
    let tok_key = refresh_token_key(presented_token);

    let result: i64 = redis::cmd("EVAL")
        .arg(ROTATE_REFRESH_TOKEN_LUA)
        .arg(2)
        .arg(&used_key)
        .arg(&tok_key)
        .arg(REFRESH_REUSE_WINDOW_SECS)
        .query_async(redis)
        .await
        .map_err(|error| {
            tracing::error!(?error, "rotate_refresh_token: EVAL failed");
            AuthError::InternalError
        })?;

    match result {
        -1 => {
            let player_id = redis.get::<_, i64>(&used_key).await.unwrap_or(0);
            tracing::warn!(player_id, "refresh token reuse detected — revoking sessions");
            revoke_player_sessions(player_id, redis).await?;
            Err(AuthError::RefreshTokenReuse)
        }
        0 => Err(AuthError::RefreshTokenInvalid),
        player_id if player_id > 0 => Ok(player_id),
        _ => {
            tracing::error!(result, "rotate_refresh_token: unexpected EVAL result");
            Err(AuthError::InternalError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_token_is_64_hex_chars() {
        let token = generate_opaque_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_error) = error {
        // Código PostgreSQL 23505 = unique_violation
        return db_error.code().as_deref() == Some("23505");
    }
    false
}
