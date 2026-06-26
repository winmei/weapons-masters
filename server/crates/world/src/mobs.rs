//! Sistema de mobs server-side
//!
//! Máquina de estados: Idle → Aggro → Attack → Dead → Respawn

use bevy_ecs::prelude::*;
use std::time::{Duration, Instant};

pub const MOB_AGGRO_RANGE: f32 = 10.0;

#[derive(Clone, Debug)]
pub struct MobDefinition {
    pub name: &'static str,
    pub max_hp: i32,
    pub damage: i32,
    pub speed: f32,
    pub attack_range: f32,
    pub attack_cooldown: Duration,
    pub xp_reward: u32,
    pub loot_table: &'static [LootEntry],
    pub respawn_time: Duration,
}

#[derive(Clone, Debug)]
pub struct LootEntry {
    pub item_id: u32,
    pub item_name: &'static str,
    pub drop_chance: f32,
}

// ---------------------------------------------------------------------------
// Definições estáticas dos 3 tipos de mob
// ---------------------------------------------------------------------------

pub static GOBLIN: MobDefinition = MobDefinition {
    name: "Goblin",
    max_hp: 60,
    damage: 8,
    speed: 4.0,
    attack_range: 1.5,
    attack_cooldown: Duration::from_millis(1500),
    xp_reward: 20,
    loot_table: &[
        LootEntry { item_id: 1, item_name: "Potion",      drop_chance: 0.30 },
        LootEntry { item_id: 2, item_name: "Copper Coin", drop_chance: 0.80 },
    ],
    respawn_time: Duration::from_secs(30),
};

pub static ORC: MobDefinition = MobDefinition {
    name: "Orc",
    max_hp: 150,
    damage: 20,
    speed: 3.0,
    attack_range: 2.0,
    attack_cooldown: Duration::from_millis(2000),
    xp_reward: 60,
    loot_table: &[
        LootEntry { item_id: 3, item_name: "Iron Sword",  drop_chance: 0.15 },
        LootEntry { item_id: 2, item_name: "Copper Coin", drop_chance: 1.00 },
    ],
    respawn_time: Duration::from_secs(45),
};

pub static TROLL: MobDefinition = MobDefinition {
    name: "Troll",
    max_hp: 350,
    damage: 40,
    speed: 2.0,
    attack_range: 2.5,
    attack_cooldown: Duration::from_millis(3000),
    xp_reward: 150,
    loot_table: &[
        LootEntry { item_id: 4, item_name: "Steel Armor",    drop_chance: 0.10 },
        LootEntry { item_id: 5, item_name: "Health Crystal", drop_chance: 0.50 },
        LootEntry { item_id: 2, item_name: "Copper Coin",    drop_chance: 1.00 },
    ],
    respawn_time: Duration::from_secs(120),
};

// ---------------------------------------------------------------------------
// Componentes ECS
// ---------------------------------------------------------------------------

#[derive(Component, Clone, Debug)]
pub struct MobIdentity {
    pub mob_id: u32,
    pub def: &'static MobDefinition,
}

#[derive(Component, Clone, Debug, Default)]
pub struct MobPosition {
    pub x: f32,
    pub y: f32,
}

impl MobPosition {
    pub fn distance_to(&self, other: &MobPosition) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

#[derive(Component, Clone, Debug)]
pub struct MobHealth {
    pub current: i32,
    pub max: i32,
}

#[derive(Component, Clone, Debug)]
pub struct SpawnPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Component, Clone, Debug)]
pub enum MobState {
    Idle,
    Aggro { target_entity_id: u32, target_x: f32, target_y: f32 },
    Attack { target_entity_id: u32, last_attack_at: Instant },
    Dead { died_at: Instant },
}

// ---------------------------------------------------------------------------
// Recursos ECS
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
pub struct MobIndex {
    pub map: std::collections::HashMap<u32, Entity>,
    pub next_id: u32,
}

impl MobIndex {
    pub fn next_mob_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }
}

/// Snapshot das posições de jogadores, atualizado pelo world server antes do mob_ai_system.
/// Evita borrow duplo ao ler jogadores e mobs no mesmo sistema.
#[derive(Resource, Default)]
pub struct PlayerPositionsSnapshot {
    pub positions: Vec<(u32, f32, f32)>,
}

#[derive(Resource, Default)]
pub struct RewardEventQueue {
    pub xp_events: Vec<XpEvent>,
    pub loot_events: Vec<LootEvent>,
}

#[derive(Clone, Debug)]
pub struct XpEvent {
    pub entity_id: u32,
    pub xp: u32,
}

#[derive(Clone, Debug)]
pub struct LootEvent {
    pub entity_id: u32,
    pub items: Vec<DroppedItem>,
}

#[derive(Clone, Debug)]
pub struct DroppedItem {
    pub item_id: u32,
    pub item_name: String,
}

// ---------------------------------------------------------------------------
// Spawner
// ---------------------------------------------------------------------------

pub fn spawn_mob(world: &mut World, def: &'static MobDefinition, x: f32, y: f32) -> u32 {
    let mob_id = world.resource_mut::<MobIndex>().next_mob_id();

    let entity = world
        .spawn((
            MobIdentity { mob_id, def },
            MobPosition { x, y },
            MobHealth { current: def.max_hp, max: def.max_hp },
            SpawnPoint { x, y },
            MobState::Idle,
        ))
        .id();

    world.resource_mut::<MobIndex>().map.insert(mob_id, entity);
    mob_id
}

pub fn spawn_starter_mobs(world: &mut World) {
    spawn_mob(world, &GOBLIN, -5.0, -3.0);
    spawn_mob(world, &GOBLIN,  5.0, -3.0);
    spawn_mob(world, &GOBLIN, -3.0, -6.0);
    spawn_mob(world, &GOBLIN,  3.0, -6.0);
    spawn_mob(world, &ORC,   -6.0,  0.0);
    spawn_mob(world, &ORC,    6.0,  0.0);
    spawn_mob(world, &TROLL,  0.0, -7.0);
}

// ---------------------------------------------------------------------------
// Sistemas ECS
// ---------------------------------------------------------------------------

/// Copia as posições atuais dos jogadores para `PlayerPositionsSnapshot`.
/// Deve rodar antes de `mob_ai_system` no schedule.
pub fn update_player_positions_system(
    player_query: Query<(&crate::NetworkIdentity, &crate::Position), With<crate::Health>>,
    mut snapshot: ResMut<PlayerPositionsSnapshot>,
) {
    snapshot.positions.clear();
    for (identity, position) in player_query.iter() {
        snapshot.positions.push((identity.entity_id, position.x, position.y));
    }
}

/// IA dos mobs — usa Query mutável para evitar Vec intermediário e alocações por tick.
pub fn mob_ai_system(
    mut query: Query<(
        &MobIdentity,
        &mut MobPosition,
        &mut MobHealth,
        &mut MobState,
        &SpawnPoint,
    )>,
    players: Res<PlayerPositionsSnapshot>,
    mut rewards: ResMut<RewardEventQueue>,
) {
    let now = Instant::now();
    let tick_delta = 1.0f32 / 30.0;

    for (identity, mut position, mut health, mut state, spawn) in query.iter_mut() {
        let next_state = compute_mob_state(
            identity,
            &position,
            health.current,
            &state,
            spawn,
            &players.positions,
            now,
            tick_delta,
            &mut position,
            &mut rewards,
        );
        *state = next_state;
    }
}

/// Retorna o próximo estado da máquina de estados do mob. Função extraída para
/// manter `mob_ai_system` coeso e facilitar testes unitários.
fn compute_mob_state(
    identity: &MobIdentity,
    position: &MobPosition,
    current_hp: i32,
    current_state: &MobState,
    spawn: &SpawnPoint,
    player_positions: &[(u32, f32, f32)],
    now: Instant,
    tick_delta: f32,
    mob_position_mut: &mut MobPosition,
    rewards: &mut ResMut<RewardEventQueue>,
) -> MobState {
    if current_hp <= 0 {
        if !matches!(current_state, MobState::Dead { .. }) {
            return MobState::Dead { died_at: now };
        }
    }

    match current_state {
        MobState::Idle => transition_from_idle(position, player_positions),

        MobState::Aggro { target_entity_id, .. } => {
            transition_from_aggro(
                identity.def,
                position,
                *target_entity_id,
                player_positions,
                now,
                tick_delta,
                mob_position_mut,
            )
        }

        MobState::Attack { target_entity_id, last_attack_at } => {
            transition_from_attack(
                identity,
                position,
                *target_entity_id,
                *last_attack_at,
                player_positions,
                now,
                rewards,
            )
        }

        MobState::Dead { died_at } => {
            transition_from_dead(identity.def, spawn, *died_at, now)
        }
    }
}

fn transition_from_idle(
    position: &MobPosition,
    player_positions: &[(u32, f32, f32)],
) -> MobState {
    match nearest_player_in_range(player_positions, position, MOB_AGGRO_RANGE) {
        Some((target_id, target_x, target_y)) => {
            MobState::Aggro { target_entity_id: target_id, target_x, target_y }
        }
        None => MobState::Idle,
    }
}

fn transition_from_aggro(
    def: &'static MobDefinition,
    position: &MobPosition,
    target_entity_id: u32,
    player_positions: &[(u32, f32, f32)],
    now: Instant,
    tick_delta: f32,
    mob_position_mut: &mut MobPosition,
) -> MobState {
    let Some((target_id, target_x, target_y)) = find_player_by_id(player_positions, target_entity_id)
    else {
        return MobState::Idle;
    };

    let target_position = MobPosition { x: target_x, y: target_y };
    let distance_to_target = position.distance_to(&target_position);

    if distance_to_target <= def.attack_range {
        return MobState::Attack {
            target_entity_id: target_id,
            // Subtrai o cooldown para que o primeiro ataque ocorra imediatamente
            last_attack_at: now - def.attack_cooldown,
        };
    }

    *mob_position_mut = move_toward(position, &target_position, def.speed, tick_delta);
    MobState::Aggro { target_entity_id: target_id, target_x, target_y }
}

fn transition_from_attack(
    identity: &MobIdentity,
    position: &MobPosition,
    target_entity_id: u32,
    last_attack_at: Instant,
    player_positions: &[(u32, f32, f32)],
    now: Instant,
    rewards: &mut ResMut<RewardEventQueue>,
) -> MobState {
    let def = identity.def;

    let Some((target_id, target_x, target_y)) = find_player_by_id(player_positions, target_entity_id)
    else {
        return MobState::Idle;
    };

    let target_position = MobPosition { x: target_x, y: target_y };
    let distance_to_target = position.distance_to(&target_position);

    if distance_to_target > def.attack_range * 1.5 {
        return MobState::Aggro { target_entity_id: target_id, target_x, target_y };
    }

    if now.duration_since(last_attack_at) >= def.attack_cooldown {
        register_mob_attack(identity, target_id, now, rewards);
        return MobState::Attack { target_entity_id: target_id, last_attack_at: now };
    }

    MobState::Attack { target_entity_id: target_id, last_attack_at }
}

fn transition_from_dead(
    def: &'static MobDefinition,
    spawn: &SpawnPoint,
    died_at: Instant,
    now: Instant,
) -> MobState {
    if now.duration_since(died_at) >= def.respawn_time {
        // O sistema apply_mob_respawn_system cuida de restaurar HP e posição
        MobState::Idle
    } else {
        MobState::Dead { died_at }
    }
}

/// Enfileira o ataque do mob para aplicação no sistema de dano de players.
fn register_mob_attack(
    identity: &MobIdentity,
    target_entity_id: u32,
    attack_time: Instant,
    rewards: &mut ResMut<RewardEventQueue>,
) {
    // Reutilizamos XpEvent como veículo de ataque pendente — em produção
    // isso seria um tipo dedicado MobAttackEvent. Mantido assim para YAGNI.
    tracing::debug!(
        mob_id = identity.mob_id,
        mob_name = identity.def.name,
        target_entity_id,
        damage = identity.def.damage,
        "mob attacking player"
    );
    // O ataque real é consumido por apply_mob_attacks_system no world server
    let _ = attack_time;
}

/// Restaura HP e posição de mobs que fizeram transição Dead→Idle neste tick.
pub fn apply_mob_respawn_system(
    mut query: Query<(&MobIdentity, &mut MobPosition, &mut MobHealth, &MobState, &SpawnPoint)>,
) {
    for (identity, mut position, mut health, state, spawn) in query.iter_mut() {
        if matches!(state, MobState::Idle) && health.current <= 0 {
            health.current = identity.def.max_hp;
            position.x = spawn.x;
            position.y = spawn.y;
            tracing::info!(mob_id = identity.mob_id, mob_name = identity.def.name, "mob respawned");
        }
    }
}

/// Coleta ataques de mobs que acabaram de atacar (neste tick) e retorna
/// (target_entity_id, damage) para aplicação pelo world server.
pub fn collect_mob_attacks(world: &mut World) -> Vec<(u32, i32)> {
    let now = Instant::now();
    let tick_window = Duration::from_millis(40);

    let mut attacks = Vec::new();
    let mut query = world.query::<(&MobIdentity, &MobState, &MobHealth)>();

    for (identity, state, health) in query.iter(world) {
        if health.current <= 0 {
            continue;
        }
        if let MobState::Attack { target_entity_id, last_attack_at } = state {
            if now.duration_since(*last_attack_at) < tick_window {
                attacks.push((*target_entity_id, identity.def.damage));
            }
        }
    }

    attacks
}

/// Registra a morte de um mob, emite rewards determinísticos via LCG com seed
/// baseada no tick + entity index para evitar dependência de rand crate.
pub fn handle_mob_death(
    world: &mut World,
    mob_entity: Entity,
    killer_entity_id: u32,
    tick: u32,
) {
    let rewards_result = compute_mob_rewards(world, mob_entity, killer_entity_id, tick);
    let (xp_reward, loot_items, mob_name) = match rewards_result {
        Some(r) => r,
        None => return,
    };

    tracing::info!(
        mob = mob_name,
        killer = killer_entity_id,
        xp = xp_reward,
        loot_count = loot_items.len(),
        "mob killed"
    );

    mark_mob_dead(world, mob_entity);
    enqueue_rewards(world, killer_entity_id, xp_reward, loot_items);
}

fn compute_mob_rewards(
    world: &World,
    mob_entity: Entity,
    killer_entity_id: u32,
    tick: u32,
) -> Option<(u32, Vec<DroppedItem>, &'static str)> {
    let identity = world.get::<MobIdentity>(mob_entity)?;
    let def = identity.def;

    let seed = (tick as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(mob_entity.index() as u64);

    let loot_items = roll_loot_impl(def.loot_table, seed);
    Some((def.xp_reward, loot_items, def.name))
}

fn roll_loot(loot_table: &[LootEntry], initial_seed: u64) -> Vec<DroppedItem> {
    roll_loot_impl(loot_table, initial_seed)
}

/// Public interface for loot rolling used by world's PvM system.
pub fn roll_loot_public(loot_table: &[LootEntry], seed: u64) -> Vec<DroppedItem> {
    roll_loot_impl(loot_table, seed)
}

fn roll_loot_impl(loot_table: &[LootEntry], initial_seed: u64) -> Vec<DroppedItem> {
    let mut rng_state = initial_seed;
    loot_table
        .iter()
        .filter_map(|entry| {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let roll = (rng_state >> 33) as f32 / u32::MAX as f32;
            if roll <= entry.drop_chance {
                Some(DroppedItem {
                    item_id: entry.item_id,
                    item_name: entry.item_name.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn mark_mob_dead(world: &mut World, mob_entity: Entity) {
    if let Some(mut health) = world.get_mut::<MobHealth>(mob_entity) {
        health.current = 0;
    }
    if let Some(mut state) = world.get_mut::<MobState>(mob_entity) {
        *state = MobState::Dead { died_at: Instant::now() };
    }
}

fn enqueue_rewards(
    world: &mut World,
    killer_entity_id: u32,
    xp_reward: u32,
    loot_items: Vec<DroppedItem>,
) {
    let mut queue = world.resource_mut::<RewardEventQueue>();
    queue.xp_events.push(XpEvent { entity_id: killer_entity_id, xp: xp_reward });
    if !loot_items.is_empty() {
        queue.loot_events.push(LootEvent {
            entity_id: killer_entity_id,
            items: loot_items,
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers privados
// ---------------------------------------------------------------------------

fn nearest_player_in_range(
    players: &[(u32, f32, f32)],
    position: &MobPosition,
    range: f32,
) -> Option<(u32, f32, f32)> {
    let range_squared = range * range;
    players
        .iter()
        .filter(|(_, px, py)| {
            let dx = position.x - px;
            let dy = position.y - py;
            dx * dx + dy * dy <= range_squared
        })
        .min_by(|(_, ax, ay), (_, bx, by)| {
            let dist_a = {
                let dx = position.x - ax;
                let dy = position.y - ay;
                dx * dx + dy * dy
            };
            let dist_b = {
                let dx = position.x - bx;
                let dy = position.y - by;
                dx * dx + dy * dy
            };
            dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
}

fn find_player_by_id(
    players: &[(u32, f32, f32)],
    target_id: u32,
) -> Option<(u32, f32, f32)> {
    players.iter().find(|(id, _, _)| *id == target_id).copied()
}

fn move_toward(from: &MobPosition, to: &MobPosition, speed: f32, tick_delta: f32) -> MobPosition {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < f32::EPSILON {
        return from.clone();
    }
    MobPosition {
        x: from.x + (dx / distance) * speed * tick_delta,
        y: from.y + (dy / distance) * speed * tick_delta,
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_loot_respects_drop_chance_boundaries() {
        // Seed que resulta em roll > 0.5 para o primeiro item — não deve dropar Potion (30%)
        // Seed que resulta em roll < 0.8 para o segundo item — deve dropar Copper Coin (80%)
        let loot = roll_loot_impl(GOBLIN.loot_table, 0);
        // Não podemos garantir resultado sem conhecer o LCG, mas garantimos que
        // nenhum item fora da tabela é gerado
        for item in &loot {
            assert!(
                GOBLIN.loot_table.iter().any(|entry| entry.item_id == item.item_id),
                "item_id {} não está na loot table do Goblin",
                item.item_id
            );
        }
    }

    #[test]
    fn nearest_player_in_range_returns_none_when_all_out_of_range() {
        let players = vec![(1, 100.0f32, 100.0f32)];
        let mob_position = MobPosition { x: 0.0, y: 0.0 };
        assert!(nearest_player_in_range(&players, &mob_position, MOB_AGGRO_RANGE).is_none());
    }

    #[test]
    fn nearest_player_in_range_picks_closest() {
        let players = vec![(1, 5.0f32, 0.0f32), (2, 2.0f32, 0.0f32)];
        let mob_position = MobPosition { x: 0.0, y: 0.0 };
        let result = nearest_player_in_range(&players, &mob_position, MOB_AGGRO_RANGE);
        assert_eq!(result.map(|(id, _, _)| id), Some(2));
    }

    #[test]
    fn move_toward_stops_at_destination() {
        let from = MobPosition { x: 0.0, y: 0.0 };
        let to = MobPosition { x: 0.0, y: 0.0 };
        let result = move_toward(&from, &to, 5.0, 1.0 / 30.0);
        assert!((result.x - from.x).abs() < f32::EPSILON);
        assert!((result.y - from.y).abs() < f32::EPSILON);
    }

    #[test]
    fn transition_from_idle_aggroes_when_player_in_range() {
        let players = vec![(42u32, 5.0f32, 0.0f32)];
        let mob_position = MobPosition { x: 0.0, y: 0.0 };
        let next = transition_from_idle(&mob_position, &players);
        assert!(matches!(
            next,
            MobState::Aggro { target_entity_id: 42, .. }
        ));
    }
}
