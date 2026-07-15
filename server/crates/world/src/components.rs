use bevy_ecs::prelude::Component;
use shared::proto::{EntityAction, Vec2};
use std::time::{Duration, Instant};

use crate::constants::*;

#[derive(Component, Clone, Copy, Debug)]
pub struct NetworkIdentity {
    pub entity_id: u32,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CharacterId {
    pub value: i64,
}

#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldMapId {
    pub value: String,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
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
pub struct LastActive {
    pub at: Instant,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Disconnected {
    pub since: Instant,
    pub timeout: Duration,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct PvPImmune;

#[derive(Component, Clone, Debug, Default)]
pub struct MovementIntent {
    pub direction: Option<Vec2>,
    pub wants_dodge: Option<Vec2>,
}

#[derive(Component, Clone, Debug)]
pub struct CombatState {
    pub cooldowns: std::collections::HashMap<u32, Instant>,
    pub casting_until: Instant,
    pub pending_skill: Option<(u32, u32)>,
    pub action: EntityAction,
    pub last_processed_input: u32,
    pub rotation: f32,
    pub collision_flags: u32,
}

impl CombatState {
    pub fn new(now: Instant) -> Self {
        let distant_past = now - Duration::from_secs(60);
        Self {
            cooldowns: std::collections::HashMap::new(),
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
pub struct DodgeState {
    pub iframe_until: Instant,
    pub cooldown_until: Instant,
}

impl DodgeState {
    pub fn new(now: Instant) -> Self {
        let distant_past = now - Duration::from_secs(60);
        Self { iframe_until: distant_past, cooldown_until: distant_past }
    }

    pub fn is_dodging_at(&self, now: Instant) -> bool {
        now < self.iframe_until
    }
    pub fn can_start_at(&self, now: Instant) -> bool {
        now >= self.cooldown_until
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct PositionHistory {
    pub buffer: [(Instant, Position); HISTORY_LEN],
    pub head: usize,
    pub filled: usize,
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
    pub fn push(&mut self, timestamp: Instant, position: Position) {
        self.buffer[self.head] = (timestamp, position);
        self.head = (self.head + 1) % HISTORY_LEN;
        self.filled = self.filled.saturating_add(1).min(HISTORY_LEN);
    }

    pub fn sample_at(&self, target_time: Instant) -> Position {
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

#[derive(Clone, Debug, PartialEq)]
pub enum StatType {
    Attack,
    Defense,
    Speed,
}

#[derive(Clone, Debug)]
pub enum EffectType {
    StatModifier { stat: StatType, value: f32, is_percentage: bool },
    DoT { damage_per_tick: i32, tick_interval: Duration, last_tick: Instant },
    Stun,
    Root,
}

#[derive(Clone, Debug)]
pub struct ActiveEffect {
    pub id: u32,
    pub name: String,
    pub effect_type: EffectType,
    pub expires_at: Instant,
    pub source_entity: Option<u32>,
}

#[derive(Component, Clone, Debug, Default)]
pub struct StatusEffects {
    pub active: Vec<ActiveEffect>,
}

#[derive(Component, Clone, Debug)]
pub struct CombatStats {
    pub base_attack: i32,
    pub base_defense: i32,
    pub base_speed: f32,
    
    pub current_attack: i32,
    pub current_defense: i32,
    pub current_speed: f32,
}

impl Default for CombatStats {
    fn default() -> Self {
        Self {
            base_attack: 10,
            base_defense: 5,
            base_speed: PLAYER_SPEED_UNITS_PER_SECOND,
            current_attack: 10,
            current_defense: 5,
            current_speed: PLAYER_SPEED_UNITS_PER_SECOND,
        }
    }
}
