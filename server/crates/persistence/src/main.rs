//! DB Sync Worker — Step 3
//! Conecta ao NATS JetStream e ao PostgreSQL, consome eventos e persiste no PG.
//!
//! Uso: NATS_URL=nats://localhost:4222 DATABASE_URL=postgres://wm:wm_dev@localhost/weapons_masters cargo run -p persistence

use persistence::run_db_sync;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wm:wm_dev@localhost/weapons_masters".to_string());

    tracing::info!(%nats_url, "Connecting to NATS...");
    let nats = async_nats::connect(&nats_url).await?;
    tracing::info!("NATS connected.");

    tracing::info!(%db_url, "Connecting to PostgreSQL...");
    let pool = sqlx::PgPool::connect(&db_url).await?;
    tracing::info!("PostgreSQL connected.");

    run_db_sync(nats, pool).await;

    Ok(())
}
