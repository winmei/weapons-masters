use async_nats::jetstream;
use sqlx::PgPool;
use futures_util::StreamExt;

// ---------------------------------------------------------------------------
// Payload structs
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct SnapshotPayload {
    // Mapeado para a coluna `id` de player_characters (BIGSERIAL)
    character_id: i64,
    player_id: i64,
    level: i32,
    experience: i64,
    hp: i32,
    max_hp: i32,
    position_x: f32,
    position_y: f32,
    position_map: String,
}

#[derive(serde::Deserialize)]
struct LevelUpPayload {
    character_id: i64,
    new_level: i32,
    new_experience: i64,
}

#[derive(serde::Deserialize)]
struct LootPayload {
    character_id: i64,
    slot: i16,
    item_id: i64,
    item_name: String,
    quantity: i32,
}

// ---------------------------------------------------------------------------
// Handlers — funções puras de persistência, sem dependência de transporte
// ---------------------------------------------------------------------------

async fn handle_snapshot(pool: &PgPool, raw: &[u8]) {
    let payload: SnapshotPayload = match serde_json::from_slice(raw) {
        Ok(p) => p,
        Err(error) => {
            tracing::error!(%error, "persistence.snapshot: failed to deserialize payload");
            return;
        }
    };

    // Snapshot de posição é raro (publicado apenas em desconexão ou a cada 5s),
    // então o UPSERT aqui não é o caminho quente.
    let result = sqlx::query(
        r#"
        UPDATE player_characters
        SET
            level        = $2,
            experience   = $3,
            hp           = $4,
            max_hp       = $5,
            position_x   = $6,
            position_y   = $7,
            position_map = $8
        WHERE id = $1
        "#,
    )
    .bind(payload.character_id)
    .bind(payload.level)
    .bind(payload.experience)
    .bind(payload.hp)
    .bind(payload.max_hp)
    .bind(payload.position_x as f64)
    .bind(payload.position_y as f64)
    .bind(&payload.position_map)
    .execute(pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() == 0 => {
            tracing::error!(
                character_id = payload.character_id,
                "persistence.snapshot: character not found — snapshot dropped"
            );
        }
        Err(error) => {
            tracing::error!(
                %error,
                character_id = payload.character_id,
                "persistence.snapshot: UPDATE failed"
            );
        }
        Ok(_) => {
            tracing::debug!(character_id = payload.character_id, "snapshot persisted");
        }
    }
}

async fn handle_levelup(pool: &PgPool, raw: &[u8]) {
    let payload: LevelUpPayload = match serde_json::from_slice(raw) {
        Ok(p) => p,
        Err(error) => {
            tracing::error!(%error, "persistence.event.levelup: failed to deserialize payload");
            return;
        }
    };

    let result = sqlx::query(
        r#"
        UPDATE player_characters
        SET level      = $2,
            experience = $3
        WHERE id = $1
        "#,
    )
    .bind(payload.character_id)
    .bind(payload.new_level)
    .bind(payload.new_experience)
    .execute(pool)
    .await;

    match result {
        Ok(r) if r.rows_affected() == 0 => {
            tracing::error!(
                character_id = payload.character_id,
                "persistence.event.levelup: character not found"
            );
        }
        Err(error) => {
            tracing::error!(
                %error,
                character_id = payload.character_id,
                "persistence.event.levelup: UPDATE failed"
            );
        }
        Ok(_) => {
            tracing::debug!(
                character_id = payload.character_id,
                new_level = payload.new_level,
                "level-up persisted"
            );
        }
    }
}

async fn handle_loot(pool: &PgPool, raw: &[u8]) {
    let payload: LootPayload = match serde_json::from_slice(raw) {
        Ok(p) => p,
        Err(error) => {
            tracing::error!(%error, "persistence.event.loot: failed to deserialize payload");
            return;
        }
    };

    // item_data armazena campos tipados como JSONB — sem dupla serialização
    let item_data = serde_json::json!({
        "item_id":   payload.item_id,
        "item_name": payload.item_name,
        "quantity":  payload.quantity,
    });

    let result = sqlx::query(
        r#"
        INSERT INTO player_inventory (character_id, slot, item_data)
        VALUES ($1, $2, $3)
        ON CONFLICT (character_id, slot) DO UPDATE SET item_data = EXCLUDED.item_data
        "#,
    )
    .bind(payload.character_id)
    .bind(payload.slot)
    .bind(&item_data)
    .execute(pool)
    .await;

    if let Err(error) = result {
        tracing::error!(
            %error,
            character_id = payload.character_id,
            slot = payload.slot,
            "persistence.event.loot: INSERT failed"
        );
    } else {
        tracing::debug!(
            character_id = payload.character_id,
            slot = payload.slot,
            "loot persisted"
        );
    }
}

// ---------------------------------------------------------------------------
// Main worker — usa JetStream para garantia de entrega at-least-once
// ---------------------------------------------------------------------------

pub async fn run_db_sync(
    nats: async_nats::Client,
    pool: PgPool,
) {
    let jetstream = jetstream::new(nats);

    let stream = match jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: "PERSISTENCE".to_string(),
            subjects: vec!["persistence.>".to_string()],
            // Retém mensagens por 24h para reprocessamento em caso de falha do worker
            max_age: std::time::Duration::from_secs(86_400),
            ..Default::default()
        })
        .await
    {
        Ok(s) => s,
        Err(error) => {
            tracing::error!(%error, "run_db_sync: failed to get or create JetStream stream");
            return;
        }
    };

    let consumer = match stream
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: Some("db-sync".to_string()),
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
    {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(%error, "run_db_sync: failed to create JetStream consumer");
            return;
        }
    };

    tracing::info!("DB sync worker started — consuming from JetStream PERSISTENCE.>");

    let mut messages = match consumer.messages().await {
        Ok(m) => m,
        Err(error) => {
            tracing::error!(%error, "run_db_sync: failed to obtain message stream");
            return;
        }
    };

    while let Some(message_result) = messages.next().await {
        let message = match message_result {
            Ok(m) => m,
            Err(error) => {
                tracing::error!(%error, "run_db_sync: error receiving message");
                continue;
            }
        };

        let subject = message.subject.as_str();
        let payload = message.payload.as_ref();

        match subject {
            "persistence.snapshot" => {
                handle_snapshot(&pool, payload).await;
            }
            "persistence.event.levelup" => {
                handle_levelup(&pool, payload).await;
            }
            "persistence.event.loot" => {
                handle_loot(&pool, payload).await;
            }
            other => {
                tracing::warn!(subject = other, "run_db_sync: unknown persistence subject");
            }
        }

        // Ack explícito — a mensagem só sai da fila após processamento bem-sucedido
        if let Err(error) = message.ack().await {
            tracing::error!(%error, subject, "run_db_sync: failed to ack message");
        }
    }

    tracing::warn!("run_db_sync: JetStream message stream ended — shutting down");
}
