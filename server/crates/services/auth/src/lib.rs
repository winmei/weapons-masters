use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task;
use sqlx::PgPool;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use serde::{Serialize, Deserialize};
use thiserror::Error;

const MAX_LOGIN_ATTEMPTS_PER_MINUTE: u64 = 5;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

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

pub fn create_jwt(player_id: i64, config: &SecurityConfig) -> Result<String, AuthError> {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + config.jwt_expiry_secs;

    let claims = Claims { sub: player_id, exp: expiry };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|error| AuthError::JwtError(error.to_string()))
}

pub fn verify_jwt(token: &str, config: &SecurityConfig) -> Result<Claims, AuthError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|error| AuthError::JwtError(error.to_string()))
}

pub async fn register(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<i64, AuthError> {
    let password_hash = hash_password(password)?;

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
) -> Result<String, AuthError> {
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

    let token = create_jwt(player_id, config)?;
    bind_session_to_ip(player_id, origin_ip, config.jwt_expiry_secs, redis).await?;

    tracing::info!(player_id, %origin_ip, "login successful");
    Ok(token)
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
        .unwrap_or(0);

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

async fn bind_session_to_ip(
    player_id: i64,
    origin_ip: IpAddr,
    expiry_secs: u64,
    redis: &mut redis::aio::MultiplexedConnection,
) -> Result<(), AuthError> {
    let session_key = format!("session:{}:ip", player_id);
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

fn is_unique_violation(error: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_error) = error {
        // Código PostgreSQL 23505 = unique_violation
        return db_error.code().as_deref() == Some("23505");
    }
    false
}
