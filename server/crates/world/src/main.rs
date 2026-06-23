use bevy_ecs::prelude::*;
use shared::proto::{
    combat_event, CombatEvent, DamageEvent, DeathEvent, DodgeResult, EntityAction, EntityState,
    InputType, PlayerInput, Vec2, WorldSnapshot,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const TICK_RATE: u64 = 30;
const TICK_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TICK_RATE);
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
const COL_TOP: u32 = 0b0010;
const COL_LEFT: u32 = 0b0100;
const COL_RIGHT: u32 = 0b1000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();
    tracing::info!("World server started. Target tick rate: {}Hz", TICK_RATE);

    let (input_tx, input_rx) = mpsc::channel::<PlayerInput>(4096);
    let (snapshot_tx, _) = broadcast::channel::<WorldSnapshot>(128);

    let gateway_handle = tokio::spawn({
        let snapshot_tx = snapshot_tx.clone();
        async move {
            if let Err(error) = gateway::run_gateway(input_tx, snapshot_tx).await {
                tracing::error!("Gateway stopped: {}", error);
            }
        }
    });

    let game_loop_handle = std::thread::spawn(move || {
        let mut world = World::new();
        world.insert_resource(NetworkChannels {
            input_rx: Mutex::new(input_rx),
            snapshot_tx,
        });
        world.insert_resource(GlobalState::default());
        world.insert_resource(EntityIndex::default());
        world.insert_resource(SpatialHash::default());
        world.insert_resource(CombatEventQueue::default());
        world.insert_resource(SnapshotCache::default());
        world.insert_resource(NetworkInputBuffer::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                process_network_inputs_system,
                cleanup_disconnected_system,
                apply_movement_and_dodge_system,
                rebuild_spatial_hash_system,
                process_combat_skills_system,
                record_position_history_system,
                build_and_send_snapshot_system,
            )
                .chain(),
        );

        loop {
            let tick_start = Instant::now();
            schedule.run(&mut world);

            let elapsed_time = tick_start.elapsed();
            if elapsed_time < TICK_DURATION {
                sleep_precise(TICK_DURATION - elapsed_time);
            } else {
                tracing::warn!("World tick overload: {:?}", elapsed_time);
            }
        }
    });

    if let Err(error) = gateway_handle.await {
        tracing::error!("Gateway task failed: {}", error);
    }

    if let Err(error) = game_loop_handle.join() {
        tracing::error!("Game loop thread panicked: {:?}", error);
    }

    Ok(())
}

#[derive(Resource)]
struct NetworkChannels {
    input_rx: Mutex<mpsc::Receiver<PlayerInput>>,
    snapshot_tx: broadcast::Sender<WorldSnapshot>,
}

#[derive(Resource, Default)]
struct GlobalState {
    tick: u32,
    last_processed_input: u32,
}

#[derive(Resource, Default)]
struct EntityIndex {
    map: HashMap<u32, Entity>,
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

#[derive(Component, Clone, Copy, Debug)]
struct NetworkIdentity {
    entity_id: u32,
}

#[derive(Component, Clone, Copy, Debug, Default)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Component, Clone, Copy, Debug)]
struct Health {
    current: i32,
    max: i32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 200,
            max: 200,
        }
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
        let past = Instant::now() - Duration::from_secs(60);
        Self {
            last_golpe_at: past,
            last_disparo_at: past,
            casting_until: past,
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
        let past = Instant::now() - Duration::from_secs(60);
        Self {
            iframe_until: past,
            cooldown_until: past,
        }
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

        let mut best_before_target: Option<(Instant, Position)> = None;
        let mut oldest = self.buffer[0];

        for idx in 0..self.filled {
            let entry = self.buffer[idx];
            if entry.0 < oldest.0 {
                oldest = entry;
            }

            if entry.0 <= target_time
                && best_before_target
                    .map(|best| entry.0 >= best.0)
                    .unwrap_or(true)
            {
                best_before_target = Some(entry);
            }
        }

        best_before_target
            .map(|(_, position)| position)
            .unwrap_or(oldest.1)
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

fn process_network_inputs_system(world: &mut World) {
    {
        let mut global_state = world.resource_mut::<GlobalState>();
        global_state.tick = global_state.tick.saturating_add(1);
    }

    let read_deadline = Instant::now() + MAX_NET_READ_BUDGET;
    let mut input_buffer = world
        .remove_resource::<NetworkInputBuffer>()
        .unwrap_or_default();
    input_buffer.inputs.clear();

    let lock_failed = {
        let input_rx_lock = world.resource::<NetworkChannels>().input_rx.lock();
        match input_rx_lock {
            Ok(mut input_rx) => {
                while Instant::now() < read_deadline {
                    match input_rx.try_recv() {
                        Ok(input) => input_buffer.inputs.push(input),
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            tracing::warn!("Gateway input channel disconnected");
                            break;
                        }
                    }
                }
                false
            }
            Err(_) => {
                tracing::warn!("World input receiver lock poisoned");
                true
            }
        }
    };

    if lock_failed {
        world.insert_resource(input_buffer);
        return;
    }

    let mut entity_index = world.remove_resource::<EntityIndex>().unwrap_or_default();

    for input in input_buffer.inputs.drain(..) {
        {
            let mut global_state = world.resource_mut::<GlobalState>();
            global_state.last_processed_input =
                global_state.last_processed_input.max(input.sequence);
        }

        let entity = match entity_index.map.get(&input.entity_id) {
            Some(entity) => *entity,
            None => {
                let entity = world
                    .spawn((
                        NetworkIdentity {
                            entity_id: input.entity_id,
                        },
                        Position::default(),
                        Health::default(),
                        CombatState::default(),
                        DodgeState::default(),
                        PositionHistory::default(),
                        MovementIntent::default(),
                        LastActive { at: Instant::now() },
                    ))
                    .id();
                entity_index.map.insert(input.entity_id, entity);
                entity
            }
        };

        let mut query = world.query::<(&mut CombatState, &mut MovementIntent, &mut LastActive)>();
        let Ok((mut combat_state, mut intent, mut last_active)) = query.get_mut(world, entity)
        else {
            continue;
        };

        last_active.at = Instant::now();
        combat_state.last_processed_input = combat_state.last_processed_input.max(input.sequence);

        match input.input_type {
            input_type if input_type == InputType::Move as i32 => {
                intent.direction = input.direction;
            }
            input_type if input_type == InputType::Stop as i32 => {
                intent.direction = None;
            }
            input_type if input_type == InputType::Dodge as i32 => {
                if let Some(dodge) = input.dodge {
                    intent.wants_dodge = dodge.direction;
                }
            }
            input_type if input_type == InputType::Skill as i32 => {
                if let Some(skill) = input.skill_use {
                    combat_state.pending_skill = Some((skill.skill_id, skill.target_entity_id));
                }
            }
            _ => {}
        }
    }

    world.insert_resource(entity_index);
    world.insert_resource(input_buffer);
}

fn cleanup_disconnected_system(world: &mut World) {
    let now = Instant::now();
    let mut stale_entities = Vec::new();

    {
        let mut query = world.query::<(Entity, &NetworkIdentity, &LastActive)>();
        for (entity, identity, last_active) in query.iter(world) {
            if now.duration_since(last_active.at) > PLAYER_INACTIVITY_TIMEOUT {
                stale_entities.push((entity, identity.entity_id));
            }
        }
    }

    if stale_entities.is_empty() {
        return;
    }

    let mut entity_index = world.remove_resource::<EntityIndex>().unwrap_or_default();

    for (entity, entity_id) in stale_entities {
        if world.despawn(entity) {
            entity_index.map.remove(&entity_id);
            tracing::info!("Player {} removed due to inactivity timeout", entity_id);
        }
    }

    world.insert_resource(entity_index);
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
    let delta_time = 1.0 / TICK_RATE as f32;

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

        if let Some(direction) = intent.direction.as_ref() {
            let length = (direction.x * direction.x + direction.y * direction.y).sqrt();
            if length > f32::EPSILON {
                let step_size = PLAYER_SPEED_UNITS_PER_SECOND * delta_time;
                position.x += (direction.x / length) * step_size;
                position.y += (direction.y / length) * step_size;
                combat_state.rotation = direction.y.atan2(direction.x);
                combat_state.action = EntityAction::Moving;
            } else {
                combat_state.action = EntityAction::Idle;
            }
        } else {
            combat_state.action = EntityAction::Idle;
        }

        resolve_world_collisions(&mut position, &mut combat_state);
    }
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
        event: Some(combat_event::Event::Dodge(DodgeResult {
            entity_id,
            success: true,
        })),
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

fn process_combat_skills_system(
    mut attackers: Query<(&NetworkIdentity, &Position, &mut CombatState)>,
    mut targets: Query<(&PositionHistory, &DodgeState, &mut Health)>,
    entity_index: Res<EntityIndex>,
    spatial_hash: Res<SpatialHash>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    let mut interactions = Vec::new();

    for (identity, position, mut combat_state) in attackers.iter_mut() {
        let Some((skill_id, target_id)) = combat_state.pending_skill.take() else {
            continue;
        };
        let Some(skill) = skill_by_id(skill_id) else {
            continue;
        };

        let last_used_at = match skill_id {
            1 => &mut combat_state.last_golpe_at,
            2 => &mut combat_state.last_disparo_at,
            _ => continue,
        };

        if now.duration_since(*last_used_at) < skill.cooldown {
            continue;
        }

        *last_used_at = now;
        combat_state.casting_until = now + skill.cast_time;
        combat_state.action = EntityAction::Casting;
        interactions.push((identity.entity_id, *position, target_id, skill));
    }

    for (attacker_id, attacker_position, target_id, skill) in interactions {
        let Some(target_entity) = entity_index.map.get(&target_id).copied() else {
            continue;
        };
        let Ok((history, dodge_state, mut health)) = targets.get_mut(target_entity) else {
            continue;
        };

        let rewind_time = now
            .checked_sub(Duration::from_millis(100))
            .unwrap_or(now)
            .max(now - Duration::from_millis(200));
        let target_position = history.sample_at(rewind_time);

        match check_hit(
            attacker_position,
            target_position,
            *dodge_state,
            skill,
            &spatial_hash,
            now,
        ) {
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
}

fn record_position_history_system(mut query: Query<(&Position, &mut PositionHistory)>) {
    let now = Instant::now();
    for (position, mut history) in query.iter_mut() {
        history.push(now, *position);
    }
}

fn build_and_send_snapshot_system(
    global_state: Res<GlobalState>,
    mut events: ResMut<CombatEventQueue>,
    channels: Res<NetworkChannels>,
    mut cache: ResMut<SnapshotCache>,
    query: Query<(&NetworkIdentity, &Position, &Health, &CombatState)>,
) {
    cache.entities.clear();

    for (identity, position, health, combat_state) in query.iter() {
        cache.entities.push(EntityState {
            entity_id: identity.entity_id,
            position: Some(Vec2 {
                x: position.x,
                y: position.y,
            }),
            rotation: combat_state.rotation,
            collision_flags: combat_state.collision_flags,
            last_processed_input: combat_state.last_processed_input,
            hp: health.current,
            max_hp: health.max,
            current_action: combat_state.action as i32,
        });
    }

    cache.entities.sort_by_key(|entity| entity.entity_id);

    let snapshot = WorldSnapshot {
        tick: global_state.tick,
        last_processed_input: global_state.last_processed_input,
        entities: cache.entities.clone(),
        combat_events: events.events.clone(),
        local_entity_id: 0,
    };

    events.events.clear();

    if channels.snapshot_tx.send(snapshot).is_err() {
        tracing::debug!("No connected clients to receive world snapshot");
    }
}

fn skill_by_id(skill_id: u32) -> Option<SkillDef> {
    match skill_id {
        1 => Some(GOLPE),
        2 => Some(DISPARO),
        _ => None,
    }
}

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

        for offset_y in -1..=1 {
            for offset_x in -1..=1 {
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
    let distance_x = attacker_position.x - target_position.x;
    let distance_y = attacker_position.y - target_position.y;
    let distance = (distance_x * distance_x + distance_y * distance_y).sqrt();

    if distance > skill.range {
        return HitResult::OutOfRange;
    }

    if spatial_hash.is_blocked(attacker_position, target_position) {
        return HitResult::Blocked;
    }

    if target_dodge.is_dodging_at(now) {
        return HitResult::Dodged;
    }

    HitResult::Hit {
        damage: skill.damage,
    }
}

fn resolve_world_collisions(position: &mut Position, combat_state: &mut CombatState) {
    let before_x = position.x;
    let before_y = position.y;

    position.x = position
        .x
        .clamp(-ARENA_LIMIT + PLAYER_HALF_EXTENT, ARENA_LIMIT - PLAYER_HALF_EXTENT);
    position.y = position
        .y
        .clamp(-ARENA_LIMIT + PLAYER_HALF_EXTENT, ARENA_LIMIT - PLAYER_HALF_EXTENT);

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

    let push_left = player_max_x - WALL_MIN_X;
    let push_right = WALL_MAX_X - player_min_x;
    let push_down = player_max_y - WALL_MIN_Y;
    let push_up = WALL_MAX_Y - player_min_y;
    let min_push = push_left.min(push_right).min(push_down).min(push_up);

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
    let steps = 12;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = from.x + (to.x - from.x) * t;
        let y = from.y + (to.y - from.y) * t;
        if (WALL_MIN_X..=WALL_MAX_X).contains(&x) && (WALL_MIN_Y..=WALL_MAX_Y).contains(&y) {
            return true;
        }
    }

    false
}

fn sleep_precise(target: Duration) {
    let deadline = Instant::now() + target;

    if target > Duration::from_millis(2) {
        std::thread::sleep(target - Duration::from_millis(2));
    }

    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_hit_returns_hit_inside_range() {
        let spatial_hash = SpatialHash::default();
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            DodgeState::default(),
            GOLPE,
            &spatial_hash,
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Hit { damage: 50 }));
    }

    #[test]
    fn check_hit_rejects_out_of_range() {
        let spatial_hash = SpatialHash::default();
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 10.0, y: 0.0 },
            DodgeState::default(),
            GOLPE,
            &spatial_hash,
            Instant::now(),
        );
        assert!(matches!(result, HitResult::OutOfRange));
    }

    #[test]
    fn check_hit_rejects_dodge_iframes() {
        let spatial_hash = SpatialHash::default();
        let now = Instant::now();
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 1.0, y: 0.0 },
            DodgeState {
                iframe_until: now + Duration::from_millis(300),
                cooldown_until: now + Duration::from_millis(1500),
            },
            GOLPE,
            &spatial_hash,
            now,
        );
        assert!(matches!(result, HitResult::Dodged));
    }

    #[test]
    fn check_hit_rejects_blocked_line_of_sight() {
        let spatial_hash = SpatialHash::default();
        let result = check_hit(
            Position { x: 0.0, y: 0.0 },
            Position { x: 0.0, y: 4.0 },
            DodgeState::default(),
            DISPARO,
            &spatial_hash,
            Instant::now(),
        );
        assert!(matches!(result, HitResult::Blocked));
    }

    #[test]
    fn spatial_hash_visits_nearby_entities_without_allocating_results() {
        let mut spatial_hash = SpatialHash::default();
        spatial_hash.clear();

        let mut world = World::new();
        let entity = world.spawn_empty().id();
        spatial_hash.insert(Position { x: 0.0, y: 0.0 }, entity);

        let mut visited = 0;
        spatial_hash.for_nearby_entities(Position { x: 0.5, y: 0.5 }, |nearby| {
            if nearby == entity {
                visited += 1;
            }
        });

        assert_eq!(visited, 1);
    }

    #[test]
    fn movement_inputs_only_move_once_per_tick() {
        let (input_tx, input_rx) = mpsc::channel::<PlayerInput>(16);
        let (snapshot_tx, _) = broadcast::channel::<WorldSnapshot>(16);
        let mut world = World::new();

        world.insert_resource(NetworkChannels {
            input_rx: Mutex::new(input_rx),
            snapshot_tx,
        });
        world.insert_resource(GlobalState::default());
        world.insert_resource(EntityIndex::default());
        world.insert_resource(SpatialHash::default());
        world.insert_resource(CombatEventQueue::default());
        world.insert_resource(SnapshotCache::default());
        world.insert_resource(NetworkInputBuffer::default());

        for sequence in 1..=8 {
            input_tx
                .try_send(PlayerInput {
                    sequence,
                    entity_id: 42,
                    input_type: InputType::Move as i32,
                    direction: Some(Vec2 { x: 1.0, y: 0.0 }),
                    skill_use: None,
                    dodge: None,
                    client_tick: sequence,
                })
                .unwrap();
        }

        let mut schedule = Schedule::default();
        schedule.add_systems(
            (process_network_inputs_system, apply_movement_and_dodge_system).chain(),
        );
        schedule.run(&mut world);

        let mut query = world.query::<(&NetworkIdentity, &Position)>();
        let (_, position) = query
            .iter(&world)
            .find(|(identity, _)| identity.entity_id == 42)
            .unwrap();

        let expected_step = PLAYER_SPEED_UNITS_PER_SECOND / TICK_RATE as f32;
        assert!((position.x - expected_step).abs() < 0.001);
    }

    #[test]
    fn cleanup_removes_inactive_entities_from_world_and_index() {
        let mut world = World::new();
        let entity_id = 77;
        let entity = world
            .spawn((
                NetworkIdentity { entity_id },
                LastActive {
                    at: Instant::now() - PLAYER_INACTIVITY_TIMEOUT - Duration::from_secs(1),
                },
            ))
            .id();

        let mut index = EntityIndex::default();
        index.map.insert(entity_id, entity);
        world.insert_resource(index);

        cleanup_disconnected_system(&mut world);

        {
            let index = world.resource::<EntityIndex>();
            assert!(!index.map.contains_key(&entity_id));
        }

        let mut query = world.query::<&NetworkIdentity>();
        assert_eq!(query.iter(&world).count(), 0);
    }
}
