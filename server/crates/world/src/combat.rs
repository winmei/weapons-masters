use std::time::{Duration, Instant};
use crate::spatial::SpatialHash;
use crate::components::{Position, DodgeState};

#[derive(Clone, Copy)]
pub struct SkillDef {
    pub range: f32,
    pub cooldown: Duration,
    pub damage: i32,
    pub cast_time: Duration,
}

pub const GOLPE: SkillDef = SkillDef {
    range: 3.0,
    cooldown: Duration::from_millis(1500),
    damage: 50,
    cast_time: Duration::ZERO,
};

pub const DISPARO: SkillDef = SkillDef {
    range: 15.0,
    cooldown: Duration::from_millis(3000),
    damage: 80,
    cast_time: Duration::from_millis(500),
};

pub fn skill_by_id(skill_id: u32) -> Option<SkillDef> {
    match skill_id {
        1 => Some(GOLPE),
        2 => Some(DISPARO),
        _ => None,
    }
}

pub enum HitResult {
    Hit { damage: i32 },
    Dodged,
    OutOfRange,
    Blocked,
}

pub fn check_hit(
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
