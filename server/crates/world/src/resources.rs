use bevy_ecs::prelude::{Entity, Resource};
use shared::proto::{CombatEvent, EntityState, PlayerInput};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use crate::components::Position;
use crate::combat::SkillDef;
use crate::publisher;

/// Receiver exclusivo da thread do game loop — sem Mutex porque não há
/// outro consumidor; o gateway só produz, nunca lê.
#[derive(Resource)]
pub struct InputReceiver(pub mpsc::Receiver<PlayerInput>);

/// Sender de snapshots pre-encoded em Arc para evitar clone/encode por conexao.
#[derive(Resource)]
pub struct SnapshotSender(pub broadcast::Sender<Arc<Vec<u8>>>);

#[derive(Resource, Default)]
pub struct GlobalState {
    pub tick: u32,
    pub last_processed_input: u32,
}

#[derive(Resource, Default)]
pub struct EntityIndex {
    pub map: HashMap<u32, Entity>,
}

#[derive(Resource, Default)]
pub struct CharacterEntityIndex {
    pub map: HashMap<i64, Entity>,
}

#[derive(Resource, Default)]
pub struct CombatEventQueue {
    pub events: Vec<CombatEvent>,
}

/// Reusable buffers for combat hit collection, avoiding per-tick allocation.
#[derive(Resource, Default)]
pub struct CombatBuffer {
    pub pending_hits: Vec<(u32, Position, u32, SkillDef)>,
    pub pending_mob_hits: Vec<(u32, Position, u32, SkillDef)>,
}

#[derive(Resource, Default)]
pub struct SnapshotCache {
    pub entities: Vec<EntityState>,
    pub mob_entities: Vec<EntityState>,
}

#[derive(Resource, Default)]
pub struct NetworkInputBuffer {
    pub inputs: Vec<PlayerInput>,
}

/// Wraps the persistence channel sender as a Bevy ECS Resource.
/// The `try_emit` method is non-blocking — safe to call inside the hot tick.
#[derive(Resource)]
pub struct PersistenceSenderResource(pub publisher::PersistenceSender);

/// Receives commands emitted after game authentication and character loading.
#[derive(Resource)]
pub struct EnterWorldReceiver(pub mpsc::Receiver<EnterWorldCommand>);

/// Complete authoritative state required to create or restore a player entity.
#[derive(Debug, Clone)]
pub struct EnterWorldCommand {
    pub entity_id: u32,
    pub character_id: i64,
    pub map_id: String,
    pub level: u32,
    pub experience: u64,
    pub current_hp: i32,
    pub maximum_hp: i32,
    pub position_x: f32,
    pub position_y: f32,
    pub rotation: f32,
}

/// Sender side cloned into the authentication state.
pub type EnterWorldSender = mpsc::Sender<EnterWorldCommand>;
