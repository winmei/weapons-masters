mod mobs;
mod experience;
mod publisher;
mod auth_service;

use bevy_ecs::prelude::*;
use shared::proto::{
    combat_event, CombatEvent, DamageEvent, DeathEvent, DodgeResult, EntityAction, EntityState,
    InputType, PlayerInput, Vec2, WorldSnapshot,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const TICK_RATE: u64 = 30;
const TICK_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE);
const TICK_DELTA: f32 = 1.0 / TICK_RATE as f32;
const MAX_NET_READ_BUDGET: Duration = Duration::from_millis(8);
const PLAYER_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
const PLAYER_SPEED_UNITS_PER_SECOND: f32 = 5.0;
const DODGE_DISTANCE: f32 = 3.0;
const DODGE_IFRAMES: Duration = Duration::from_millis(300);
const DODGE_COOLDOWN: Duration = Duration::from_millis(1500);
const HISTORY_LEN: usize = 12;
const PLAYER_HALF_EXTENT: f32 = 0.5;
const ARENA_LIMIT: f32 = 8.0;
const WALL_MIN_X: f32 = -2.5;
const WALL_MAX_X: f32 = 2.5;
const WALL_MIN_Y: f32 = 2.5;
const WALL_MAX_Y: f32 = 3.0;
const GRID_WIDTH: usize = 64;
const GRID_HEIGHT: usize = 64;
const CELL_COUNT: usize = GRID_WIDTH * GRID_HEIGHT;
const SPATIAL_NONE: u32 = u32::MAX;
const DEFAULT_SPATIAL_CAPACITY: usize = 2048;

const COL_BOTTOM: u32 = 0b0001;
const COL_TOP: u32    = 0b0010;
const COL_LEFT: u32   = 0b0100;
const COL_RIGHT: u32  = 0b1000;

/// Publish a full character snapshot every N ticks (≈ 30s at 30Hz).
/// Tolerant to small loss — position/HP will be resent on the next interval.
const SNAPSHOT_INTERVAL_TICKS: u32 = 900;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    tracing::info!("World server started. Target tick rate: {}Hz", TICK_RATE);

    // ---------------------------------------------------------------------------
    // Auth service — init PostgreSQL + Redis + SecurityConfig from env.
    // Fails fast at startup if JWT_SECRET is missing or weak (per $wm-persistence-auth).
    // ---------------------------------------------------------------------------

    // Channel for auth task → ECS world: after login, the auth handler pushes
    // the real character_id so the tick loop can set it on the player entity.
    let (char_assign_tx, char_assign_rx) =
        mpsc::channel::<CharacterAssignment>(256);

    let auth_state = match auth_service::init_auth_state(char_assign_tx).await {
        Ok(s) => {
            tracing::info!("Auth service ready");
            Some(s)
        }
        Err(e) => {
            tracing::warn!("Auth service unavailable — login/register disabled: {e}");
            None
        }
    };

    // ---------------------------------------------------------------------------
    // Persistence publisher — NATS channel between tick loop and JetStream.
    // The hot tick uses try_send (non-blocking). The publisher task awaits acks.
    // JoinHandle saved so we can await drain on graceful shutdown.
    // ---------------------------------------------------------------------------
    let nats_url = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let (persistence_tx, persistence_rx) = publisher::persistence_channel();

    let publisher_handle = tokio::spawn(publisher::run_persistence_publisher(
        nats_url.clone(),
        persistence_rx,
    ));
    tracing::info!(%nats_url, "Persistence publisher task spawned");

    // ---------------------------------------------------------------------------
    // Auth gateway on port 8081 — only started when auth state is available.
    // ---------------------------------------------------------------------------
    if let Some(ref state) = auth_state {
        let handlers = auth_service::build_auth_handlers(std::sync::Arc::clone(state));
        tokio::spawn(gateway::run_auth_gateway(handlers));
        tracing::info!("Auth gateway spawned on TCP {}", gateway::DEFAULT_AUTH_PORT);
    }

    // ---------------------------------------------------------------------------
    // Game networking channels
    // ---------------------------------------------------------------------------
    let (input_tx, input_rx) = mpsc::channel::<PlayerInput>(4096);
    let (snapshot_tx, _) = broadcast::channel::<Arc<WorldSnapshot>>(128);

    let mut gateway_handle = tokio::spawn({
        let snapshot_tx = snapshot_tx.clone();
        // Build the game-auth handler only when auth state is available.
        // When None, GameAuthPacket is silently accepted but ignored (anonymous / dev mode).
        let game_auth = auth_state.as_ref().map(|state| {
            auth_service::build_game_auth_handler(Arc::clone(state))
        });
        async move {
            let config = gateway::GatewayConfig { game_auth };
            if let Err(error) = gateway::run_gateway_with_config(input_tx, snapshot_tx, config).await {
                tracing::error!("Gateway stopped: {}", error);
            }
        }
    });

    // ---------------------------------------------------------------------------
    // Shutdown flag — shared between async main and the game OS thread.
    // AtomicBool avoids Mutex overhead in the hot tick check path.
    // ---------------------------------------------------------------------------
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let game_shutdown_flag = Arc::clone(&shutdown_flag);

    // ---------------------------------------------------------------------------
    // Game loop — dedicated OS thread; owns the ECS world exclusively.
    // PersistenceSender is moved in and stored as a Resource so ECS systems
    // can call try_emit() without any async or mutex involvement.
    // ---------------------------------------------------------------------------
    let game_loop_handle = std::thread::spawn(move || {
        let mut world = World::new();
        world.insert_resource(InputReceiver(input_rx));
        world.insert_resource(SnapshotSender(snapshot_tx));
        world.insert_resource(GlobalState::default());
        world.insert_resource(EntityIndex::default());
        world.insert_resource(SpatialHash::default());
        world.insert_resource(CombatEventQueue::default());
        world.insert_resource(SnapshotCache::default());
        world.insert_resource(NetworkInputBuffer::default());
        world.insert_resource(mobs::MobIndex::default());
        world.insert_resource(mobs::RewardEventQueue::default());
        world.insert_resource(mobs::PlayerPositionsSnapshot::default());
        world.insert_resource(experience::LevelUpEventQueue::default());
        // PersistenceSender: try_send is non-blocking — safe in the hot tick.
        world.insert_resource(PersistenceSenderResource(persistence_tx));
        // CharacterIdReceiver: drained each tick to bind DB IDs to player entities.
        world.insert_resource(CharacterIdReceiver(char_assign_rx));
        world.insert_resource(PendingCharacterAssignments::default());
        mobs::spawn_starter_mobs(&mut world);

        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                process_network_inputs_system,
                apply_character_assignments_system,
                cleanup_disconnected_system,
                apply_movement_and_dodge_system,
                rebuild_spatial_hash_system,
                process_player_combat_skills_system,
                process_player_vs_mob_system,
                clear_unresolved_skills_system,
                mobs::update_player_positions_system,
                mobs::mob_ai_system,
                apply_mob_attacks_system,
                mobs::apply_mob_respawn_system,
                record_position_history_system,
                experience::experience_system,
                emit_persistence_events_system,
                build_and_broadcast_snapshot_system,
            )
                .chain(),
        );

        loop {
            let tick_start = Instant::now();
            schedule.run(&mut world);

            // Check shutdown flag after every full tick (max delay ≈ 33ms at 30Hz).
            // Avoids I/O or locking inside the hot path; Acquire matches the Release
            // store in main after the signal is received.
            if game_shutdown_flag.load(Ordering::Acquire) {
                tracing::info!("Game loop: shutdown signal received — flushing player snapshots");
                flush_all_players_on_shutdown(&mut world);
                tracing::info!("Game loop: graceful shutdown complete");
                // world drops here → PersistenceSenderResource drops → publisher channel closes
                break;
            }

            let elapsed = tick_start.elapsed();
            if elapsed < TICK_DURATION {
                sleep_precise(TICK_DURATION - elapsed);
            } else {
                tracing::warn!("World tick overload: {:?}", elapsed);
            }
        }
    });

    // ---------------------------------------------------------------------------
    // Wait for shutdown signal (SIGTERM / Ctrl+C) or unexpected gateway exit.
    // ---------------------------------------------------------------------------
    tokio::select! {
        _ = await_shutdown_signal() => {
            tracing::info!("Received shutdown signal — initiating graceful shutdown");
        }
        _ = &mut gateway_handle => {
            tracing::warn!("Gateway task ended unexpectedly — initiating shutdown");
        }
    }

    // Signal the game loop to stop after its current tick.
    shutdown_flag.store(true, Ordering::Release);

    // Wait for game loop to complete final flush and exit.
    // Blocks for at most one tick duration (≈ 33ms) — acceptable on shutdown path.
    if let Err(error) = game_loop_handle.join() {
        tracing::error!("Game loop thread panicked during shutdown: {:?}", error);
    }
    tracing::info!("Game loop exited — waiting for persistence publisher to drain NATS");

    // The game loop dropped PersistenceSenderResource → publisher channel is now closed.
    // Await the publisher task so all in-flight NATS publishes complete before exit.
    if let Err(error) = publisher_handle.await {
        tracing::error!("Persistence publisher task failed: {:?}", error);
    }

    tracing::info!("Weapons Masters server shut down cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Recursos
// ---------------------------------------------------------------------------

/// Receiver exclusivo da thread do game loop — sem Mutex porque não há
/// outro consumidor; o gateway só produz, nunca lê.
#[derive(Resource)]
struct InputReceiver(mpsc::Receiver<PlayerInput>);

/// Sender de snapshots Arc para evitar clone do payload por tick.
#[derive(Resource)]
struct SnapshotSender(broadcast::Sender<Arc<WorldSnapshot>>);

#[derive(Resource, Default)]
pub struct GlobalState {
    pub tick: u32,
    last_processed_input: u32,
}

#[derive(Resource, Default)]
pub struct EntityIndex {
    pub map: HashMap<u32, Entity>,
}

#[derive(Resource, Default)]
struct CombatEventQueue {
    events: Vec<CombatEvent>,
}

#[derive(Resource, Default)]
struct SnapshotCache {
    entities: Vec<EntityState>,
}

#[derive(Resource, Default)]
struct NetworkInputBuffer {
    inputs: Vec<PlayerInput>,
}

/// Wraps the persistence channel sender as a Bevy ECS Resource.
/// The `try_emit` method is non-blocking — safe to call inside the hot tick.
#[derive(Resource)]
struct PersistenceSenderResource(publisher::PersistenceSender);

/// Receives (entity_id, character_id, player_progress) assignments from the
/// auth task after a successful login. Drained each tick in process_network_inputs_system.
#[derive(Resource)]
struct CharacterIdReceiver(mpsc::Receiver<CharacterAssignment>);

/// Holds `CharacterAssignment`s that arrived before the player entity was
/// created in the ECS (i.e., before the first `PlayerInput` from that connection).
///
/// `apply_character_assignments_system` retries these each tick until the
/// entity appears (created by `process_network_inputs_system`).
#[derive(Resource, Default)]
struct PendingCharacterAssignments {
    pending: std::collections::HashMap<u32, CharacterAssignment>,
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

// ---------------------------------------------------------------------------
// Componentes
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Copy, Debug)]
pub struct NetworkIdentity {
    pub entity_id: u32,
}

/// The real PostgreSQL `player_characters.id` for this player entity.
/// Set when the player logs in via auth; used by persistence to write correct rows.
/// If zero, the entity was spawned without auth (dev/test mode — entity_id is used as surrogate).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CharacterId {
    pub value: i64,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

impl Default for Health {
    fn default() -> Self {
        Self { current: 200, max: 200 }
    }
}

#[derive(Component, Clone, Copy, Debug)]
struct LastActive {
    at: Instant,
}

#[derive(Component, Clone, Debug, Default)]
struct MovementIntent {
    direction: Option<Vec2>,
    wants_dodge: Option<Vec2>,
}

#[derive(Component, Clone, Copy, Debug)]
struct CombatState {
    last_golpe_at: Instant,
    last_disparo_at: Instant,
    casting_until: Instant,
    pending_skill: Option<(u32, u32)>,
    action: EntityAction,
    last_processed_input: u32,
    rotation: f32,
    collision_flags: u32,
}

impl Default for CombatState {
    fn default() -> Self {
        let distant_past = Instant::now() - Duration::from_secs(60);
        Self {
            last_golpe_at: distant_past,
            last_disparo_at: distant_past,
            casting_until: distant_past,
            pending_skill: None,
            action: EntityAction::Idle,
            last_processed_input: 0,
            rotation: 0.0,
            collision_flags: 0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug)]
struct DodgeState {
    iframe_until: Instant,
    cooldown_until: Instant,
}

impl Default for DodgeState {
    fn default() -> Self {
        let distant_past = Instant::now() - Duration::from_secs(60);
        Self { iframe_until: distant_past, cooldown_until: distant_past }
    }
}

impl DodgeState {
    fn is_dodging_at(&self, now: Instant) -> bool {
        now < self.iframe_until
    }
    fn can_start_at(&self, now: Instant) -> bool {
        now >= self.cooldown_until
    }
}

#[derive(Component, Clone, Copy, Debug)]
struct PositionHistory {
    buffer: [(Instant, Position); HISTORY_LEN],
    head: usize,
    filled: usize,
}

impl Default for PositionHistory {
    fn default() -> Self {
        Self {
            buffer: [(Instant::now(), Position::default()); HISTORY_LEN],
            head: 0,
            filled: 0,
        }
    }
}

impl PositionHistory {
    fn push(&mut self, timestamp: Instant, position: Position) {
        self.buffer[self.head] = (timestamp, position);
        self.head = (self.head + 1) % HISTORY_LEN;
        self.filled = self.filled.saturating_add(1).min(HISTORY_LEN);
    }

    fn sample_at(&self, target_time: Instant) -> Position {
        if self.filled == 0 {
            return Position::default();
        }
        let mut best_before: Option<(Instant, Position)> = None;
        let mut oldest = self.buffer[0];
        for idx in 0..self.filled {
            let entry = self.buffer[idx];
            if entry.0 < oldest.0 {
                oldest = entry;
            }
            if entry.0 <= target_time {
                if best_before.map(|b| entry.0 >= b.0).unwrap_or(true) {
                    best_before = Some(entry);
                }
            }
        }
        best_before.map(|(_, p)| p).unwrap_or(oldest.1)
    }
}

#[derive(Clone, Copy)]
struct SkillDef {
    range: f32,
    cooldown: Duration,
    damage: i32,
    cast_time: Duration,
}

const GOLPE: SkillDef = SkillDef {
    range: 3.0,
    cooldown: Duration::from_millis(1500),
    damage: 50,
    cast_time: Duration::ZERO,
};

const DISPARO: SkillDef = SkillDef {
    range: 15.0,
    cooldown: Duration::from_millis(3000),
    damage: 80,
    cast_time: Duration::from_millis(500),
};

fn skill_by_id(skill_id: u32) -> Option<SkillDef> {
    match skill_id {
        1 => Some(GOLPE),
        2 => Some(DISPARO),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Sistemas ECS
// ---------------------------------------------------------------------------

/// Drena inputs da fila MPSC sem Mutex — o receiver é exclusivo desta thread.
fn process_network_inputs_system(
    mut input_rx: ResMut<InputReceiver>,
    mut global_state: ResMut<GlobalState>,
    mut entity_index: ResMut<EntityIndex>,
    mut buffer: ResMut<NetworkInputBuffer>,
    mut commands: Commands,
    mut query: Query<(&mut CombatState, &mut MovementIntent, &mut LastActive)>,
) {
    global_state.tick = global_state.tick.saturating_add(1);
    buffer.inputs.clear();

    let read_deadline = Instant::now() + MAX_NET_READ_BUDGET;
    loop {
        if Instant::now() >= read_deadline {
            break;
        }
        match input_rx.0.try_recv() {
            Ok(input) => buffer.inputs.push(input),
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                tracing::warn!("Gateway input channel disconnected");
                break;
            }
        }
    }

    for input in buffer.inputs.drain(..) {
        global_state.last_processed_input =
            global_state.last_processed_input.max(input.sequence);

        let entity = *entity_index.map.entry(input.entity_id).or_insert_with(|| {
            commands.spawn((
                NetworkIdentity { entity_id: input.entity_id },
                CharacterId::default(),
                Position::default(),
                Health::default(),
                CombatState::default(),
                DodgeState::default(),
                PositionHistory::default(),
                MovementIntent::default(),
                experience::PlayerProgress::default(),
                LastActive { at: Instant::now() },
            )).id()
        });

        let Ok((mut combat_state, mut intent, mut last_active)) = query.get_mut(entity) else {
            continue;
        };

        last_active.at = Instant::now();
        combat_state.last_processed_input =
            combat_state.last_processed_input.max(input.sequence);

        apply_input_to_intent(&input, &mut intent, &mut combat_state);
    }
}

fn apply_input_to_intent(
    input: &PlayerInput,
    intent: &mut MovementIntent,
    combat_state: &mut CombatState,
) {
    match input.input_type {
        t if t == InputType::Move as i32 => {
            intent.direction = input.direction.clone();
        }
        t if t == InputType::Stop as i32 => {
            intent.direction = None;
        }
        t if t == InputType::Dodge as i32 => {
            if let Some(dodge) = &input.dodge {
                intent.wants_dodge = dodge.direction.clone();
            }
        }
        t if t == InputType::Skill as i32 => {
            if let Some(skill) = &input.skill_use {
                combat_state.pending_skill = Some((skill.skill_id, skill.target_entity_id));
            }
        }
        _ => {}
    }
}

/// Drains `CharacterIdReceiver` and applies DB character data to player entities.
///
/// Assignments that arrive before the entity is created (i.e., `GameAuthPacket`
/// processed before the first `PlayerInput`) are buffered in
/// `PendingCharacterAssignments` and retried each tick until the entity appears.
///
/// Runs immediately after `process_network_inputs_system` so that assignments
/// that arrive in the same tick as the first input are applied immediately.
fn apply_character_assignments_system(
    mut char_rx: ResMut<CharacterIdReceiver>,
    mut pending: ResMut<PendingCharacterAssignments>,
    entity_index: Res<EntityIndex>,
    mut query: Query<(
        &mut CharacterId,
        &mut Position,
        &mut Health,
        &mut experience::PlayerProgress,
    )>,
) {
    // Drain the channel into the pending map (latest wins on key collision).
    loop {
        match char_rx.0.try_recv() {
            Ok(assignment) => {
                pending.pending.insert(assignment.entity_id, assignment);
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                tracing::warn!("character assignment channel disconnected");
                break;
            }
        }
    }

    // Try to apply all pending assignments. Collect keys first to avoid
    // borrowing `pending` while mutably accessing query components.
    let entity_ids: Vec<u32> = pending.pending.keys().copied().collect();

    for entity_id in entity_ids {
        let Some(&entity) = entity_index.map.get(&entity_id) else {
            // Entity not yet spawned — first PlayerInput will create it;
            // the assignment stays buffered and will be retried next tick.
            tracing::debug!(entity_id, "character assignment buffered — entity not yet in ECS");
            continue;
        };

        // Entity exists: consume the assignment and apply it.
        let assignment = pending.pending.remove(&entity_id).expect("entity_id was just found in pending");

        let Ok((mut char_id, mut pos, mut health, mut progress)) =
            query.get_mut(entity)
        else {
            continue;
        };

        char_id.value     = assignment.character_id;
        pos.x             = assignment.position_x;
        pos.y             = assignment.position_y;
        health.current    = assignment.hp;
        progress.level    = assignment.level;
        progress.experience = assignment.experience;

        tracing::info!(
            entity_id,
            character_id = assignment.character_id,
            level = assignment.level,
            "character data applied to ECS entity"
        );
    }
}

fn cleanup_disconnected_system(
    mut commands: Commands,
    mut entity_index: ResMut<EntityIndex>,
    query: Query<(Entity, &NetworkIdentity, &LastActive)>,
) {
    let now = Instant::now();
    for (entity, identity, last_active) in query.iter() {
        if now.duration_since(last_active.at) > PLAYER_INACTIVITY_TIMEOUT {
            entity_index.map.remove(&identity.entity_id);
            commands.entity(entity).despawn();
            tracing::info!(entity_id = identity.entity_id, "player removed due to inactivity");
        }
    }
}

fn apply_movement_and_dodge_system(
    mut query: Query<(
        &NetworkIdentity,
        &mut Position,
        &mut CombatState,
        &mut DodgeState,
        &mut MovementIntent,
        &Health,
    )>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    for (identity, mut position, mut combat_state, mut dodge_state, mut intent, health) in
        query.iter_mut()
    {
        combat_state.collision_flags = 0;

        if health.current <= 0 {
            combat_state.action = EntityAction::Dead;
            intent.direction = None;
            intent.wants_dodge = None;
            continue;
        }

        if let Some(dodge_direction) = intent.wants_dodge.take() {
            apply_dodge_intent(
                identity.entity_id,
                dodge_direction,
                now,
                &mut position,
                &mut combat_state,
                &mut dodge_state,
                &mut events,
            );
        }

        if dodge_state.is_dodging_at(now) {
            combat_state.action = EntityAction::Dodging;
            resolve_world_collisions(&mut position, &mut combat_state);
            continue;
        }

        if now < combat_state.casting_until {
            combat_state.action = EntityAction::Casting;
            resolve_world_collisions(&mut position, &mut combat_state);
            continue;
        }

        apply_movement_intent(&mut position, &mut combat_state, &intent);
        resolve_world_collisions(&mut position, &mut combat_state);
    }
}

fn apply_movement_intent(
    position: &mut Position,
    combat_state: &mut CombatState,
    intent: &MovementIntent,
) {
    if let Some(direction) = intent.direction.as_ref() {
        let length = (direction.x * direction.x + direction.y * direction.y).sqrt();
        if length > f32::EPSILON {
            let step = PLAYER_SPEED_UNITS_PER_SECOND * TICK_DELTA;
            position.x += (direction.x / length) * step;
            position.y += (direction.y / length) * step;
            combat_state.rotation = direction.y.atan2(direction.x);
            combat_state.action = EntityAction::Moving;
            return;
        }
    }
    combat_state.action = EntityAction::Idle;
}

fn apply_dodge_intent(
    entity_id: u32,
    direction: Vec2,
    now: Instant,
    position: &mut Position,
    combat_state: &mut CombatState,
    dodge_state: &mut DodgeState,
    events: &mut CombatEventQueue,
) {
    if !dodge_state.can_start_at(now) {
        return;
    }
    let length = (direction.x * direction.x + direction.y * direction.y).sqrt();
    if length <= f32::EPSILON {
        return;
    }
    position.x += (direction.x / length) * DODGE_DISTANCE;
    position.y += (direction.y / length) * DODGE_DISTANCE;
    combat_state.rotation = direction.y.atan2(direction.x);
    combat_state.action = EntityAction::Dodging;
    dodge_state.iframe_until = now + DODGE_IFRAMES;
    dodge_state.cooldown_until = now + DODGE_COOLDOWN;
    events.events.push(CombatEvent {
        event: Some(combat_event::Event::Dodge(DodgeResult { entity_id, success: true })),
    });
}

fn rebuild_spatial_hash_system(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &Position, &Health)>,
) {
    spatial_hash.clear();
    for (entity, position, health) in query.iter() {
        if health.current > 0 {
            spatial_hash.insert(*position, entity);
        }
    }
}

fn process_player_combat_skills_system(
    mut attackers: Query<(&NetworkIdentity, &Position, &mut CombatState)>,
    mut targets: Query<(&PositionHistory, &DodgeState, &mut Health)>,
    entity_index: Res<EntityIndex>,
    mob_index: Res<mobs::MobIndex>,
    spatial_hash: Res<SpatialHash>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    let mut pending_hits: Vec<(u32, Position, u32, SkillDef)> = Vec::new();

    for (identity, position, mut combat_state) in attackers.iter_mut() {
        let Some((skill_id, target_id)) = combat_state.pending_skill else {
            continue;
        };
        // Only consume here if target is a player — mob targets handled by process_player_vs_mob_system
        if !entity_index.map.contains_key(&target_id) {
            continue;
        }
        combat_state.pending_skill = None;

        let Some(skill) = skill_by_id(skill_id) else {
            tracing::warn!(skill_id, entity_id = identity.entity_id, "unknown skill_id");
            continue;
        };
        if !skill_cooldown_elapsed(skill_id, &combat_state, skill.cooldown, now) {
            continue;
        }
        update_skill_cooldown(skill_id, &mut combat_state, skill, now);
        pending_hits.push((identity.entity_id, *position, target_id, skill));
    }

    for (attacker_id, attacker_position, target_id, skill) in pending_hits {
        let Some(target_entity) = entity_index.map.get(&target_id).copied() else {
            continue;
        };
        let Ok((history, dodge_state, mut health)) = targets.get_mut(target_entity) else {
            continue;
        };
        let rewound_position = rewind_target_position(history, now);
        let hit = check_hit(attacker_position, rewound_position, *dodge_state, skill, &spatial_hash, now);
        apply_hit_result(hit, attacker_id, target_id, &mut health, &mut events);
    }
}

/// Allows players to attack mobs. Mobs use `MobHealth` instead of `Health`,
/// so this is a separate system from player-vs-player combat.
fn process_player_vs_mob_system(
    mut attackers: Query<(&NetworkIdentity, &Position, &mut CombatState)>,
    mut mob_query: Query<(&mobs::MobIdentity, &mobs::MobPosition, &mut mobs::MobHealth, &mut mobs::MobState)>,
    mob_index: Res<mobs::MobIndex>,
    spatial_hash: Res<SpatialHash>,
    global_state: Res<GlobalState>,
    mut events: ResMut<CombatEventQueue>,
    mut rewards: ResMut<mobs::RewardEventQueue>,
) {
    let now = Instant::now();
    let mut pending_mob_hits: Vec<(u32, Position, u32, SkillDef)> = Vec::new();

    for (identity, position, mut combat_state) in attackers.iter_mut() {
        let Some((skill_id, target_id)) = combat_state.pending_skill else {
            continue;
        };
        // Only process if the target is a mob (mob IDs don't exist in entity_index)
        // Mob IDs are assigned from MobIndex, player IDs from NEXT_WEBSOCKET_ENTITY_ID.
        // We attempt to find in mob_index to distinguish.
        if mob_index.map.contains_key(&target_id) {
            combat_state.pending_skill = None;
            let Some(skill) = skill_by_id(skill_id) else {
                tracing::warn!(skill_id, entity_id = identity.entity_id, "unknown skill_id (mob target)");
                continue;
            };
            if !skill_cooldown_elapsed(skill_id, &combat_state, skill.cooldown, now) {
                continue;
            }
            update_skill_cooldown(skill_id, &mut combat_state, skill, now);
            pending_mob_hits.push((identity.entity_id, *position, target_id, skill));
        }
    }

    for (attacker_id, attacker_pos, mob_id, skill) in pending_mob_hits {
        let Some(&mob_entity) = mob_index.map.get(&mob_id) else { continue };
        let Ok((mob_identity, mob_pos, mut mob_health, mut mob_state)) = mob_query.get_mut(mob_entity) else {
            continue;
        };
        if mob_health.current <= 0 {
            continue;
        }

        let mob_as_position = Position { x: mob_pos.x, y: mob_pos.y };
        let dx = attacker_pos.x - mob_pos.x;
        let dy = attacker_pos.y - mob_pos.y;
        let distance = (dx * dx + dy * dy).sqrt();

        if distance > skill.range {
            continue;
        }
        if spatial_hash.is_blocked(attacker_pos, mob_as_position) {
            continue;
        }

        let damage = skill.damage;
        mob_health.current = (mob_health.current - damage).max(0);

        events.events.push(CombatEvent {
            event: Some(combat_event::Event::Damage(DamageEvent {
                source_entity_id: attacker_id,
                target_entity_id: mob_id,
                damage,
                remaining_hp: mob_health.current,
            })),
        });

        tracing::debug!(
            attacker = attacker_id,
            mob_id,
            mob_name = mob_identity.def.name,
            damage,
            remaining_hp = mob_health.current,
            "player hit mob"
        );

        if mob_health.current == 0 {
            *mob_state = mobs::MobState::Dead { died_at: now };

            events.events.push(CombatEvent {
                event: Some(combat_event::Event::Death(DeathEvent {
                    entity_id: mob_id,
                    killer_id: attacker_id,
                })),
            });

            // Roll loot and queue XP reward
            let seed = (global_state.tick as u64)
                .wrapping_mul(6364136223846793005)
                .wrapping_add(mob_entity.index() as u64);

            let loot = mobs::roll_loot_public(mob_identity.def.loot_table, seed);
            rewards.xp_events.push(mobs::XpEvent {
                entity_id: attacker_id,
                xp: mob_identity.def.xp_reward,
            });
            if !loot.is_empty() {
                rewards.loot_events.push(mobs::LootEvent {
                    entity_id: attacker_id,
                    items: loot,
                });
            }

            tracing::info!(
                killer = attacker_id,
                mob_id,
                mob_name = mob_identity.def.name,
                xp = mob_identity.def.xp_reward,
                "mob killed"
            );
        }
    }
}

/// Clears any `pending_skill` that wasn't consumed by PvP or PvM systems.
/// This prevents a skill intent from lingering across ticks if the target is invalid.
fn clear_unresolved_skills_system(mut query: Query<&mut CombatState>) {
    for mut combat_state in query.iter_mut() {
        combat_state.pending_skill = None;
    }
}

fn skill_cooldown_elapsed(
    skill_id: u32,
    combat_state: &CombatState,
    cooldown: Duration,
    now: Instant,
) -> bool {
    let last_used = match skill_id {
        1 => combat_state.last_golpe_at,
        2 => combat_state.last_disparo_at,
        _ => return false,
    };
    now.duration_since(last_used) >= cooldown
}

fn update_skill_cooldown(
    skill_id: u32,
    combat_state: &mut CombatState,
    skill: SkillDef,
    now: Instant,
) {
    match skill_id {
        1 => combat_state.last_golpe_at = now,
        2 => combat_state.last_disparo_at = now,
        _ => {}
    }
    combat_state.casting_until = now + skill.cast_time;
    combat_state.action = EntityAction::Casting;
}

fn rewind_target_position(history: &PositionHistory, now: Instant) -> Position {
    // 100ms de rewind para compensar latência típica de ~50ms RTT
    let rewind_time = now
        .checked_sub(Duration::from_millis(100))
        .unwrap_or(now)
        .max(now - Duration::from_millis(200));
    history.sample_at(rewind_time)
}

fn apply_hit_result(
    hit: HitResult,
    attacker_id: u32,
    target_id: u32,
    health: &mut Health,
    events: &mut CombatEventQueue,
) {
    match hit {
        HitResult::Hit { damage } => {
            health.current = (health.current - damage).max(0);
            events.events.push(CombatEvent {
                event: Some(combat_event::Event::Damage(DamageEvent {
                    source_entity_id: attacker_id,
                    target_entity_id: target_id,
                    damage,
                    remaining_hp: health.current,
                })),
            });
            if health.current == 0 {
                events.events.push(CombatEvent {
                    event: Some(combat_event::Event::Death(DeathEvent {
                        entity_id: target_id,
                        killer_id: attacker_id,
                    })),
                });
            }
        }
        HitResult::Dodged => {
            events.events.push(CombatEvent {
                event: Some(combat_event::Event::Dodge(DodgeResult {
                    entity_id: target_id,
                    success: true,
                })),
            });
        }
        HitResult::OutOfRange | HitResult::Blocked => {}
    }
}

/// Aplica o dano de ataques de mobs nos jogadores.
/// Corrige o bloqueador: mob_attack_results nunca era chamado no schedule original.
fn apply_mob_attacks_system(
    mut player_health_query: Query<(&NetworkIdentity, &mut Health)>,
    entity_index: Res<EntityIndex>,
    mob_query: Query<(&mobs::MobIdentity, &mobs::MobState, &mobs::MobHealth)>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    // Janela de um tick: ataques que ocorreram neste ciclo de 33ms
    let attack_window = Duration::from_millis(40);

    for (mob_identity, mob_state, mob_health) in mob_query.iter() {
        if mob_health.current <= 0 {
            continue;
        }
        let mobs::MobState::Attack { target_entity_id, last_attack_at } = mob_state else {
            continue;
        };
        if now.duration_since(*last_attack_at) >= attack_window {
            continue;
        }

        let Some(target_entity) = entity_index.map.get(target_entity_id).copied() else {
            continue;
        };
        let Ok((player_identity, mut health)) = player_health_query.get_mut(target_entity) else {
            continue;
        };
        if health.current <= 0 {
            continue;
        }

        let damage = mob_identity.def.damage;
        health.current = (health.current - damage).max(0);

        events.events.push(CombatEvent {
            event: Some(combat_event::Event::Damage(DamageEvent {
                source_entity_id: mob_identity.mob_id,
                target_entity_id: player_identity.entity_id,
                damage,
                remaining_hp: health.current,
            })),
        });

        if health.current == 0 {
            events.events.push(CombatEvent {
                event: Some(combat_event::Event::Death(DeathEvent {
                    entity_id: player_identity.entity_id,
                    killer_id: mob_identity.mob_id,
                })),
            });
        }
    }
}

fn record_position_history_system(mut query: Query<(&Position, &mut PositionHistory)>) {
    let now = Instant::now();
    for (position, mut history) in query.iter_mut() {
        history.push(now, *position);
    }
}

/// Drains the reward and level-up queues and emits persistence events to NATS
/// via the non-blocking channel. Runs before `build_and_broadcast_snapshot_system`
/// so both systems see the queues populated by `experience_system`.
///
/// Per `$wm-persistence-auth`:
/// - Level-up and loot are critical → emitted immediately.
/// - Character position snapshots are tolerant → emitted every SNAPSHOT_INTERVAL_TICKS.
/// - try_emit is non-blocking; a full channel logs a warning and drops the event.
fn emit_persistence_events_system(
    global_state: Res<GlobalState>,
    persistence: Res<PersistenceSenderResource>,
    levelup_queue: Res<experience::LevelUpEventQueue>,
    reward_queue: Res<mobs::RewardEventQueue>,
    player_query: Query<(&NetworkIdentity, &Position, &Health, &experience::PlayerProgress, &CharacterId)>,
) {
    use publisher::{CharacterSnapshotData, LevelUpEventData, LootDropEventData, PersistenceEvent};

    // Critical: level-up events — emit immediately so the DB is updated even if
    // the server crashes before the next periodic snapshot.
    for notification in &levelup_queue.events {
        // Look up the real character_id for this entity_id.
        // If the player logged in without auth (dev mode), character_id.value == 0
        // and we skip persistence rather than writing to row 0.
        let character_id = find_character_id(&player_query, notification.entity_id);
        if character_id == 0 {
            tracing::debug!(
                entity_id = notification.entity_id,
                "emit_persistence: no character_id for level-up — skipping (dev mode or not yet loaded)"
            );
            continue;
        }
        persistence.0.try_emit(PersistenceEvent::LevelUp(LevelUpEventData {
            character_id,
            new_level: notification.new_level as i32,
            new_experience: notification.new_experience as i64,
        }));
    }

    // Critical: loot drops — each item gets its own event with a sequential slot.
    for loot_event in &reward_queue.loot_events {
        let character_id = find_character_id(&player_query, loot_event.entity_id);
        if character_id == 0 {
            tracing::debug!(
                entity_id = loot_event.entity_id,
                "emit_persistence: no character_id for loot — skipping (dev mode or not yet loaded)"
            );
            continue;
        }
        for (slot_offset, item) in loot_event.items.iter().enumerate() {
            persistence.0.try_emit(PersistenceEvent::LootDrop(LootDropEventData {
                character_id,
                slot: slot_offset as i16,
                item_id: item.item_id as i64,
                item_name: item.item_name.clone(),
                quantity: 1,
            }));
        }
    }

    // Tolerant: periodic full snapshot for every online player with a known character_id.
    if global_state.tick % SNAPSHOT_INTERVAL_TICKS == 0 && global_state.tick > 0 {
        for (identity, position, health, progress, char_id) in player_query.iter() {
            if char_id.value == 0 {
                continue; // Skip players without a real DB character (dev/test)
            }
            persistence.0.try_emit(PersistenceEvent::CharacterSnapshot(CharacterSnapshotData {
                character_id: char_id.value,
                player_id: char_id.value, // same as character for Step 3 single-character flow
                level: progress.level as i32,
                experience: progress.experience as i64,
                hp: health.current,
                max_hp: health.max,
                position_x: position.x,
                position_y: position.y,
                position_map: "starter".to_string(),
            }));
            let _ = identity; // entity_id used for routing only, not for DB writes
        }
    }
}

/// Resolves entity_id → character_id from the player query.
/// Returns 0 if the entity has not been assigned a real DB character yet.
fn find_character_id(
    player_query: &Query<(&NetworkIdentity, &Position, &Health, &experience::PlayerProgress, &CharacterId)>,
    entity_id: u32,
) -> i64 {
    for (identity, _, _, _, char_id) in player_query.iter() {
        if identity.entity_id == entity_id {
            return char_id.value;
        }
    }
    0
}

/// Emits a final `CharacterSnapshot` for every authenticated online player.
///
/// Called by the game loop on graceful shutdown (SIGTERM / Ctrl+C) before
/// the world is dropped. Uses `try_emit` (non-blocking) because:
///   - Channel capacity is 4096 >> typical online player count.
///   - Dropping `world` after this function closes the sender, signalling
///     the publisher task to drain and exit.
///
/// Per `$wm-infra-ops`: shutdown must flush state before the process exits.
fn flush_all_players_on_shutdown(world: &mut World) {
    use publisher::{CharacterSnapshotData, PersistenceEvent};

    // Phase 1: collect snapshot data (immutable borrow via QueryState).
    // QueryState does not borrow `world` — only iter() does, for its duration.
    let mut q = world
        .query::<(&Position, &Health, &experience::PlayerProgress, &CharacterId)>();

    let snapshots: Vec<CharacterSnapshotData> = q
        .iter(world)
        .filter(|(_, _, _, char_id)| char_id.value != 0)
        .map(|(pos, health, progress, char_id)| CharacterSnapshotData {
            character_id: char_id.value,
            player_id:    char_id.value,
            level:        progress.level as i32,
            experience:   progress.experience as i64,
            hp:           health.current,
            max_hp:       health.max,
            position_x:   pos.x,
            position_y:   pos.y,
            position_map: "starter".to_string(),
        })
        .collect();
    // iter() borrow ends here — world is free again.

    let count = snapshots.len();
    if count == 0 {
        tracing::info!("graceful shutdown: no authenticated players online — nothing to flush");
        return;
    }
    tracing::info!(count, "graceful shutdown: emitting final player snapshots to NATS");

    // Phase 2: emit snapshots (separate immutable borrow of world via get_resource).
    if let Some(sender) = world.get_resource::<PersistenceSenderResource>() {
        for snapshot in snapshots {
            if !sender.0.try_emit(PersistenceEvent::CharacterSnapshot(snapshot)) {
                tracing::warn!(
                    "graceful shutdown: persistence channel full — snapshot dropped; \
                     consider increasing PERSISTENCE_CHANNEL_CAPACITY"
                );
            }
        }
    } else {
        tracing::warn!("graceful shutdown: PersistenceSenderResource missing — no flush performed");
    }
}

/// Waits for SIGTERM (Unix/Docker) or Ctrl+C (all platforms).
///
/// Docker and Kubernetes send SIGTERM on `docker stop` / pod termination.
/// Ctrl+C (SIGINT) is for local development.
/// Both trigger the same graceful shutdown path.
async fn await_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    %e,
                    "Failed to install SIGTERM handler — only Ctrl+C will trigger graceful shutdown"
                );
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received SIGINT (Ctrl+C)");
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
            }
        }
    }
    #[cfg(not(unix))]
    {
        // Windows: only Ctrl+C is available via tokio::signal.
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Received Ctrl+C");
    }
}

fn build_and_broadcast_snapshot_system(
    global_state: Res<GlobalState>,
    mut events: ResMut<CombatEventQueue>,
    snapshot_sender: Res<SnapshotSender>,
    mut cache: ResMut<SnapshotCache>,
    query: Query<(&NetworkIdentity, &Position, &Health, &CombatState)>,
    mob_query: Query<(&mobs::MobIdentity, &mobs::MobPosition, &mobs::MobHealth, &mobs::MobState)>,
    mut levelup_queue: ResMut<experience::LevelUpEventQueue>,
    mut rewards: ResMut<mobs::RewardEventQueue>,
) {
    // Player entities
    cache.entities.clear();
    for (identity, position, health, combat_state) in query.iter() {
        cache.entities.push(EntityState {
            entity_id: identity.entity_id,
            position: Some(Vec2 { x: position.x, y: position.y }),
            rotation: combat_state.rotation,
            collision_flags: combat_state.collision_flags,
            last_processed_input: combat_state.last_processed_input,
            hp: health.current,
            max_hp: health.max,
            current_action: combat_state.action as i32,
        });
    }
    cache.entities.sort_by_key(|entity| entity.entity_id);

    // Mob entities — reuse the same EntityState shape so the client can render them
    let mut mob_states: Vec<EntityState> = Vec::new();
    for (identity, position, health, state) in mob_query.iter() {
        let action = match state {
            mobs::MobState::Idle => EntityAction::Idle as i32,
            mobs::MobState::Aggro { .. } => EntityAction::Moving as i32,
            mobs::MobState::Attack { .. } => EntityAction::Casting as i32,
            mobs::MobState::Dead { .. } => EntityAction::Dead as i32,
        };
        mob_states.push(EntityState {
            entity_id: identity.mob_id,
            position: Some(Vec2 { x: position.x, y: position.y }),
            rotation: 0.0,
            collision_flags: 0,
            last_processed_input: 0,
            hp: health.current,
            max_hp: health.max,
            current_action: action,
        });
    }

    // Level-up events → proto LevelUpEvent
    use shared::proto::LevelUpEvent as ProtoLevelUp;
    let level_up_events: Vec<ProtoLevelUp> = levelup_queue.events.drain(..)
        .map(|e| ProtoLevelUp {
            entity_id: e.entity_id,
            new_level: e.new_level as i32,
            new_experience: e.new_experience as i64,
        })
        .collect();

    // Loot drops → proto LootDrop (slot assigned incrementally per player)
    use shared::proto::{ItemData, LootDrop};
    let mut loot_drops: Vec<LootDrop> = Vec::new();
    for loot_event in rewards.loot_events.drain(..) {
        for (slot_offset, item) in loot_event.items.iter().enumerate() {
            loot_drops.push(LootDrop {
                entity_id: loot_event.entity_id,
                slot: slot_offset as i32,
                item: Some(ItemData {
                    item_id: item.item_id,
                    item_name: item.item_name.clone(),
                    quantity: 1,
                }),
            });
        }
    }

    // Arc evita clone do payload para cada receiver conectado
    let snapshot = Arc::new(WorldSnapshot {
        tick: global_state.tick,
        last_processed_input: global_state.last_processed_input,
        entities: cache.entities.clone(),
        combat_events: std::mem::take(&mut events.events),
        local_entity_id: 0,
        level_up_events,
        loot_drops,
        mob_entities: mob_states,
    });

    if snapshot_sender.0.send(snapshot).is_err() {
        tracing::debug!("No connected clients — snapshot dropped");
    }
}

// ---------------------------------------------------------------------------
// SpatialHash
// ---------------------------------------------------------------------------

#[derive(Default, Resource)]
struct SpatialHash {
    cell_size: f32,
    heads: Vec<u32>,
    next: Vec<u32>,
    entities: Vec<Entity>,
}

impl SpatialHash {
    fn clear(&mut self) {
        if self.cell_size == 0.0 {
            self.cell_size = 2.0;
            self.heads = vec![SPATIAL_NONE; CELL_COUNT];
            self.next = Vec::with_capacity(DEFAULT_SPATIAL_CAPACITY);
            self.entities = Vec::with_capacity(DEFAULT_SPATIAL_CAPACITY);
        }
        self.heads.fill(SPATIAL_NONE);
        self.next.clear();
        self.entities.clear();
    }

    fn insert(&mut self, position: Position, entity: Entity) {
        let cell = self.cell_index(position);
        let slot = self.entities.len();
        self.entities.push(entity);
        self.next.push(self.heads[cell]);
        self.heads[cell] = slot as u32;
    }

    fn cell_index(&self, position: Position) -> usize {
        let cell_x = (position.x / self.cell_size).floor() as i32;
        let cell_y = (position.y / self.cell_size).floor() as i32;
        Self::linear_index(cell_x, cell_y)
    }

    fn linear_index(cell_x: i32, cell_y: i32) -> usize {
        let x = cell_x.rem_euclid(GRID_WIDTH as i32) as usize;
        let y = cell_y.rem_euclid(GRID_HEIGHT as i32) as usize;
        y * GRID_WIDTH + x
    }

    fn is_blocked(&self, from: Position, to: Position) -> bool {
        segment_intersects_wall(from, to)
    }

    fn for_nearby_entities(&self, position: Position, mut visit: impl FnMut(Entity)) {
        if self.cell_size == 0.0 || self.heads.is_empty() {
            return;
        }
        let cell_x = (position.x / self.cell_size).floor() as i32;
        let cell_y = (position.y / self.cell_size).floor() as i32;
        for offset_y in -1i32..=1 {
            for offset_x in -1i32..=1 {
                let cell = Self::linear_index(cell_x + offset_x, cell_y + offset_y);
                let mut current = self.heads[cell];
                while current != SPATIAL_NONE {
                    let slot = current as usize;
                    visit(self.entities[slot]);
                    current = self.next[slot];
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hit detection — função pura, testável sem World
// ---------------------------------------------------------------------------

enum HitResult {
    Hit { damage: i32 },
    Dodged,
    OutOfRange,
    Blocked,
}

fn check_hit(
    attacker_position: Position,
    target_position: Position,
    target_dodge: DodgeState,
    skill: SkillDef,
    spatial_hash: &SpatialHash,
    now: Instant,
) -> HitResult {
    let dx = attacker_position.x - target_position.x;
    let dy = attacker_position.y - target_position.y;
    let distance = (dx * dx + dy * dy).sqrt();

    if distance > skill.range {
        return HitResult::OutOfRange;
    }
    if spatial_hash.is_blocked(attacker_position, target_position) {
        return HitResult::Blocked;
    }
    if target_dodge.is_dodging_at(now) {
        return HitResult::Dodged;
    }
    HitResult::Hit { damage: skill.damage }
}

// ---------------------------------------------------------------------------
// Colisão e utilidades
// ---------------------------------------------------------------------------

fn resolve_world_collisions(position: &mut Position, combat_state: &mut CombatState) {
    let before_x = position.x;
    let before_y = position.y;

    position.x = position.x.clamp(
        -ARENA_LIMIT + PLAYER_HALF_EXTENT,
        ARENA_LIMIT - PLAYER_HALF_EXTENT,
    );
    position.y = position.y.clamp(
        -ARENA_LIMIT + PLAYER_HALF_EXTENT,
        ARENA_LIMIT - PLAYER_HALF_EXTENT,
    );

    if position.x != before_x {
        combat_state.collision_flags |= COL_LEFT | COL_RIGHT;
    }
    if position.y != before_y {
        combat_state.collision_flags |= COL_BOTTOM | COL_TOP;
    }

    let player_min_x = position.x - PLAYER_HALF_EXTENT;
    let player_max_x = position.x + PLAYER_HALF_EXTENT;
    let player_min_y = position.y - PLAYER_HALF_EXTENT;
    let player_max_y = position.y + PLAYER_HALF_EXTENT;

    let overlaps_wall = player_max_x > WALL_MIN_X
        && player_min_x < WALL_MAX_X
        && player_max_y > WALL_MIN_Y
        && player_min_y < WALL_MAX_Y;

    if !overlaps_wall {
        return;
    }

    let push_left  = player_max_x - WALL_MIN_X;
    let push_right = WALL_MAX_X   - player_min_x;
    let push_down  = player_max_y - WALL_MIN_Y;
    let push_up    = WALL_MAX_Y   - player_min_y;
    let min_push   = push_left.min(push_right).min(push_down).min(push_up);

    if min_push == push_left {
        position.x -= push_left;
        combat_state.collision_flags |= COL_LEFT;
    } else if min_push == push_right {
        position.x += push_right;
        combat_state.collision_flags |= COL_RIGHT;
    } else if min_push == push_down {
        position.y -= push_down;
        combat_state.collision_flags |= COL_BOTTOM;
    } else {
        position.y += push_up;
        combat_state.collision_flags |= COL_TOP;
    }
}

fn segment_intersects_wall(from: Position, to: Position) -> bool {
    for step in 0..=12 {
        let t = step as f32 / 12.0;
        let x = from.x + (to.x - from.x) * t;
        let y = from.y + (to.y - from.y) * t;
        if (WALL_MIN_X..=WALL_MAX_X).contains(&x) && (WALL_MIN_Y..=WALL_MAX_Y).contains(&y) {
            return true;
        }
    }
    false
}

/// Híbrido sleep+spin para precisão de timing sem consumir 100% de CPU na janela inteira.
/// O spin apenas para a diferença residual de <2ms, minimizando jitter no deadline do tick.
fn sleep_precise(remaining: Duration) {
    let deadline = Instant::now() + remaining;
    if remaining > Duration::from_millis(2) {
        std::thread::sleep(remaining - Duration::from_millis(2));
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spatial_hash() -> SpatialHash {
        SpatialHash::default()
    }

    #[test]
    fn check_hit_returns_hit_inside_range() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            DodgeState::default(),
            GOLPE,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Hit { damage: 50 }));
    }

    #[test]
    fn check_hit_rejects_out_of_range() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 10.0, y: 0.0 },
            DodgeState::default(),
            GOLPE,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::OutOfRange));
    }

    #[test]
    fn check_hit_rejects_dodge_iframes() {
        let now = Instant::now();
        let active_dodge = DodgeState {
            iframe_until: now + Duration::from_millis(300),
            cooldown_until: now + Duration::from_millis(1500),
        };
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            active_dodge,
            GOLPE,
            &make_spatial_hash(),
            now,
        );
        assert!(matches!(result, HitResult::Dodged));
    }

    #[test]
    fn check_hit_rejects_blocked_line_of_sight() {
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 0.0, y: 4.0 },
            DodgeState::default(),
            DISPARO,
            &make_spatial_hash(),
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Blocked));
    }

    #[test]
    fn spatial_hash_visits_nearby_entity() {
        let mut spatial_hash = SpatialHash::default();
        spatial_hash.clear();
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        spatial_hash.insert(Position { x: 0.0, y: 0.0 }, entity);
        let mut visited = 0;
        spatial_hash.for_nearby_entities(Position { x: 0.5, y: 0.5 }, |e| {
            if e == entity { visited += 1; }
        });
        assert_eq!(visited, 1);
    }

    #[test]
    fn apply_input_to_intent_move_sets_direction() {
        let mut intent = MovementIntent::default();
        let mut combat_state = CombatState::default();
        let input = PlayerInput {
            sequence: 1,
            input_type: InputType::Move as i32,
            direction: Some(Vec2 { x: 1.0, y: 0.0 }),
            ..Default::default()
        };
        apply_input_to_intent(&input, &mut intent, &mut combat_state);
        assert!(intent.direction.is_some());
    }

    #[test]
    fn apply_input_to_intent_stop_clears_direction() {
        let mut intent = MovementIntent {
            direction: Some(Vec2 { x: 1.0, y: 0.0 }),
            ..Default::default()
        };
        let mut combat_state = CombatState::default();
        let input = PlayerInput {
            sequence: 2,
            input_type: InputType::Stop as i32,
            ..Default::default()
        };
        apply_input_to_intent(&input, &mut intent, &mut combat_state);
        assert!(intent.direction.is_none());
    }

    #[test]
    fn position_history_sample_returns_closest_past_entry() {
        let mut history = PositionHistory::default();
        let base = Instant::now();
        history.push(base, Position { x: 1.0, y: 0.0 });
        history.push(base + Duration::from_millis(50), Position { x: 2.0, y: 0.0 });
        let sampled = history.sample_at(base + Duration::from_millis(25));
        assert!((sampled.x - 1.0).abs() < f32::EPSILON);
    }
}
