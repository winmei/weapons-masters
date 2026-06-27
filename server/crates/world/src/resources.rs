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

/// Receives (entity_id, character_id, player_progress) assignments from the
/// auth task after a successful login. Drained each tick in process_network_inputs_system.
#[derive(Resource)]
pub struct CharacterIdReceiver(pub mpsc::Receiver<CharacterAssignment>);

/// Holds `CharacterAssignment`s that arrived before the player entity was
/// created in the ECS (i.e., before the first `PlayerInput` from that connection).
///
/// `apply_character_assignments_system` retries these each tick until the
/// entity appears (created by `process_network_inputs_system`).
#[derive(Resource, Default)]
pub struct PendingCharacterAssignments {
    pub pending: std::collections::HashMap<u32, CharacterAssignment>,
}

/// Sent by the auth task to set the DB character on an existing ECS player entity.
pub struct CharacterAssignment {
    pub entity_id: u32,
    pub character_id: i64,
    pub level: u32,
    pub experience: u64,
    pub hp: i32,
    pub position_x: f32,
    pub position_y: f32,
}

/// Sender side — cloned into the auth state so auth handlers can push assignments.
pub type CharacterAssignmentSender = mpsc::Sender<CharacterAssignment>;
