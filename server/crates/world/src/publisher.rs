//! Persistence publisher — Step 3
//!
//! Bridge between the ECS tick loop and NATS JetStream.
//!
//! # Design
//!
//! The hot game-loop thread NEVER touches NATS. It drains `RewardEventQueue`
//! and `LevelUpEventQueue` at the end of each tick and sends lightweight
//! `PersistenceEvent` values through an `mpsc` channel. A dedicated Tokio
//! task owns the NATS client and drains the channel, publishing to JetStream.
//!
//! Critical events (level-up, loot) are published immediately.
//! Periodic snapshots are published every `SNAPSHOT_INTERVAL_TICKS` ticks.
//!
//! Per `$wm-persistence-auth`:
//! - At-least-once delivery via JetStream (consumer ACK on the worker side).
//! - No I/O in the hot tick.
//! - Events classified: critical (immediate) vs. tolerant (periodic snapshot).
//! - `character_id` is required for every DB write; during Step 3 we use
//!   `entity_id` directly as a surrogate until the login flow loads real IDs.

use serde::Serialize;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Public event type
// ---------------------------------------------------------------------------

/// All persistence events the tick loop can emit.
/// Each variant maps to a specific NATS subject consumed by `run_db_sync`.
#[derive(Debug, Clone)]
pub enum PersistenceEvent {
    /// Player gained XP and levelled up — immediate, critical.
    LevelUp(LevelUpEventData),
    /// Player received loot from a mob kill — immediate, critical.
    LootDrop(LootDropEventData),
    /// Periodic position + HP snapshot for a player — tolerant, batched.
    CharacterSnapshot(CharacterSnapshotData),
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelUpEventData {
    /// During Step 3 the entity_id IS the character_id surrogate.
    /// Step 4 will replace with the real character_id from PG.
    pub character_id: i64,
    pub new_level: i32,
    pub new_experience: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LootDropEventData {
    pub character_id: i64,
    /// Slot is assigned sequentially per kill; persistence worker deduplicates
    /// via ON CONFLICT on (character_id, slot).
    pub slot: i16,
    pub item_id: i64,
    pub item_name: String,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterSnapshotData {
    pub character_id: i64,
    pub player_id: i64,
    pub level: i32,
    pub experience: i64,
    pub hp: i32,
    pub max_hp: i32,
    pub position_x: f32,
    pub position_y: f32,
    pub position_map: String,
}

// ---------------------------------------------------------------------------
// Channel types
// ---------------------------------------------------------------------------

/// Capacity: 4096 slots. A full channel means the publisher task is behind;
/// the tick loop uses `try_send` and logs a warning — it never blocks.
pub const PERSISTENCE_CHANNEL_CAPACITY: usize = 4096;

pub fn persistence_channel() -> (PersistenceSender, PersistenceReceiver) {
    let (tx, rx) = mpsc::channel(PERSISTENCE_CHANNEL_CAPACITY);
    (PersistenceSender(tx), PersistenceReceiver(rx))
}

/// Sender half — owned by the ECS resource, cloned cheaply per use.
pub struct PersistenceSender(pub mpsc::Sender<PersistenceEvent>);

impl PersistenceSender {
    /// Non-blocking send. Returns `false` if the channel is full.
    /// The hot tick calls this; it MUST NOT await.
    pub fn try_emit(&self, event: PersistenceEvent) -> bool {
        match self.0.try_send(event) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!("persistence channel full — event dropped");
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Publisher task died; log once and continue.
                tracing::error!("persistence channel closed — publisher task may have crashed");
                false
            }
        }
    }
}

pub struct PersistenceReceiver(pub mpsc::Receiver<PersistenceEvent>);

// ---------------------------------------------------------------------------
// Publisher task — runs in Tokio, owns the NATS connection
// ---------------------------------------------------------------------------

/// Subjects must match what `run_db_sync` in the `persistence` crate expects.
const SUBJECT_LEVELUP: &str = "persistence.event.levelup";
const SUBJECT_LOOT: &str = "persistence.event.loot";
const SUBJECT_SNAPSHOT: &str = "persistence.snapshot";

/// Connects to NATS JetStream and runs until the sender side is dropped.
/// Publishes each `PersistenceEvent` to the correct subject.
///
/// Caller spawns this with `tokio::spawn(run_persistence_publisher(...))`.
pub async fn run_persistence_publisher(
    nats_url: String,
    mut receiver: PersistenceReceiver,
) {
    let nats = match async_nats::connect(&nats_url).await {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(%error, %nats_url, "persistence publisher: failed to connect to NATS");
            return;
        }
    };

    let jetstream = async_nats::jetstream::new(nats);

    // Ensure the stream exists before publishing. If it was already created by
    // `run_db_sync`, `get_or_create_stream` returns the existing one.
    match jetstream
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: "PERSISTENCE".to_string(),
            subjects: vec!["persistence.>".to_string()],
            max_age: std::time::Duration::from_secs(86_400),
            ..Default::default()
        })
        .await
    {
        Ok(_) => tracing::info!("persistence publisher: JetStream stream ready"),
        Err(error) => {
            tracing::error!(%error, "persistence publisher: failed to ensure JetStream stream");
            // Continue anyway; publish calls will fail gracefully below.
        }
    }

    tracing::info!(%nats_url, "persistence publisher started");

    while let Some(event) = receiver.0.recv().await {
        publish_event(&jetstream, event).await;
    }

    tracing::info!("persistence publisher: channel closed, shutting down");
}

async fn publish_event(
    jetstream: &async_nats::jetstream::Context,
    event: PersistenceEvent,
) {
    let (subject, payload) = match serialize_event(event) {
        Some(pair) => pair,
        None => return, // serialization error already logged
    };

    match jetstream.publish(subject, payload.into()).await {
        Ok(ack_future) => {
            // Await the JetStream ack to confirm the broker received the message.
            // This is async I/O fully outside the hot tick — safe here.
            if let Err(error) = ack_future.await {
                tracing::error!(%error, "persistence publisher: JetStream ack failed");
            }
        }
        Err(error) => {
            tracing::error!(%error, "persistence publisher: publish failed");
        }
    }
}

fn serialize_event(event: PersistenceEvent) -> Option<(&'static str, Vec<u8>)> {
    match event {
        PersistenceEvent::LevelUp(data) => {
            let bytes = match serde_json::to_vec(&data) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(?e, "persistence publisher: failed to serialize LevelUp");
                    return None;
                }
            };
            Some((SUBJECT_LEVELUP, bytes))
        }
        PersistenceEvent::LootDrop(data) => {
            let bytes = match serde_json::to_vec(&data) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(?e, "persistence publisher: failed to serialize LootDrop");
                    return None;
                }
            };
            Some((SUBJECT_LOOT, bytes))
        }
        PersistenceEvent::CharacterSnapshot(data) => {
            let bytes = match serde_json::to_vec(&data) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(?e, "persistence publisher: failed to serialize CharacterSnapshot");
                    return None;
                }
            };
            Some((SUBJECT_SNAPSHOT, bytes))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — pure functions, no I/O required
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levelup_serializes_expected_json_fields() {
        let data = LevelUpEventData {
            character_id: 42,
            new_level: 5,
            new_experience: 150,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"character_id\":42"), "missing character_id: {json}");
        assert!(json.contains("\"new_level\":5"), "missing new_level: {json}");
        assert!(json.contains("\"new_experience\":150"), "missing new_experience: {json}");
    }

    #[test]
    fn loot_drop_serializes_expected_json_fields() {
        let data = LootDropEventData {
            character_id: 7,
            slot: 3,
            item_id: 2,
            item_name: "Copper Coin".to_string(),
            quantity: 1,
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"character_id\":7"), "missing character_id: {json}");
        assert!(json.contains("\"slot\":3"), "missing slot: {json}");
        assert!(json.contains("\"item_name\":\"Copper Coin\""), "missing item_name: {json}");
    }

    #[test]
    fn snapshot_serializes_position_map_field() {
        let data = CharacterSnapshotData {
            character_id: 1,
            player_id: 1,
            level: 2,
            experience: 50,
            hp: 180,
            max_hp: 200,
            position_x: 3.5,
            position_y: -1.2,
            position_map: "starter".to_string(),
        };
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("\"position_map\":\"starter\""), "missing position_map: {json}");
    }

    #[test]
    fn try_emit_returns_false_when_channel_full() {
        // Channel capacity of 1 — second send must fail.
        let (tx, _rx) = mpsc::channel::<PersistenceEvent>(1);
        let sender = PersistenceSender(tx);
        let event = PersistenceEvent::LevelUp(LevelUpEventData {
            character_id: 1,
            new_level: 2,
            new_experience: 0,
        });
        // First send fills the single slot.
        assert!(sender.try_emit(event.clone()));
        // Second send should fail — channel is full and receiver hasn't drained.
        assert!(!sender.try_emit(event));
    }

    #[test]
    fn serialize_event_levelup_returns_correct_subject() {
        let event = PersistenceEvent::LevelUp(LevelUpEventData {
            character_id: 99,
            new_level: 10,
            new_experience: 900,
        });
        let (subject, bytes) = serialize_event(event).unwrap();
        assert_eq!(subject, "persistence.event.levelup");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn serialize_event_snapshot_returns_correct_subject() {
        let event = PersistenceEvent::CharacterSnapshot(CharacterSnapshotData {
            character_id: 5,
            player_id: 5,
            level: 1,
            experience: 0,
            hp: 200,
            max_hp: 200,
            position_x: 0.0,
            position_y: 0.0,
            position_map: "starter".to_string(),
        });
        let (subject, bytes) = serialize_event(event).unwrap();
        assert_eq!(subject, "persistence.snapshot");
        assert!(!bytes.is_empty());
    }
}
