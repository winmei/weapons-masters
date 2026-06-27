use bevy_ecs::prelude::*;
use std::time::{Duration, Instant};
use shared::proto::{combat_event, CombatEvent, DamageEvent, DeathEvent, DodgeResult, EntityAction};
use crate::components::{CombatState, Disconnected, DodgeState, Health, NetworkIdentity, Position, PositionHistory, PvPImmune};
use crate::resources::{CombatBuffer, CombatEventQueue, EntityIndex, GlobalState};
use crate::spatial::SpatialHash;
use crate::mobs;
use crate::combat::{check_hit, skill_by_id, HitResult, SkillDef};

pub fn process_player_combat_skills_system(
    mut attackers: Query<(&NetworkIdentity, &Position, &mut CombatState)>,
    mut targets: Query<(&PositionHistory, &DodgeState, &mut Health, Option<&PvPImmune>)>,
    entity_index: Res<EntityIndex>,
    spatial_hash: Res<SpatialHash>,
    mut events: ResMut<CombatEventQueue>,
    mut buffer: ResMut<CombatBuffer>,
) {
    let now = Instant::now();
    buffer.pending_hits.clear();

    for (identity, position, mut combat_state) in attackers.iter_mut() {
        let Some((_skill_id, target_id)) = peek_skill(&*combat_state) else {
            continue;
        };
        if !entity_index.map.contains_key(&target_id) {
            continue; // not a player — PvM system handles it
        }
        let Some((_skill_id, _target_id, skill)) = try_consume_skill(&mut combat_state, now) else {
            continue;
        };
        buffer.pending_hits.push((identity.entity_id, *position, target_id, skill));
    }

    for (attacker_id, attacker_position, target_id, skill) in buffer.pending_hits.drain(..) {
        let Some(target_entity) = entity_index.map.get(&target_id).copied() else {
            continue;
        };
        let Ok((history, dodge_state, mut health, pvp_immune)) = targets.get_mut(target_entity) else {
            continue;
        };
        if pvp_immune.is_some() {
            tracing::debug!(
                attacker = attacker_id,
                target = target_id,
                "attack blocked — target is PvP immune (disconnected)"
            );
            continue;
        }
        let rewound_position = rewind_target_position(history, now);
        let hit = check_hit(attacker_position, rewound_position, *dodge_state, skill, &spatial_hash, now);
        apply_hit_result(hit, attacker_id, target_id, &mut health, &mut events);
    }
}

pub fn process_player_vs_mob_system(
    mut attackers: Query<(&NetworkIdentity, &Position, &mut CombatState)>,
    mut mob_query: Query<(&mobs::MobIdentity, &mobs::MobPosition, &mut mobs::MobHealth, &mut mobs::MobState)>,
    mob_index: Res<mobs::MobIndex>,
    spatial_hash: Res<SpatialHash>,
    global_state: Res<GlobalState>,
    mut events: ResMut<CombatEventQueue>,
    mut rewards: ResMut<mobs::RewardEventQueue>,
    mut buffer: ResMut<CombatBuffer>,
) {
    let now = Instant::now();
    buffer.pending_mob_hits.clear();

    for (identity, position, mut combat_state) in attackers.iter_mut() {
        let Some((_skill_id, target_id)) = peek_skill(&*combat_state) else {
            continue;
        };
        if !mob_index.map.contains_key(&target_id) {
            continue;
        }
        let Some((_skill_id, _target_id, skill)) = try_consume_skill(&mut combat_state, now) else {
            continue;
        };
        buffer.pending_mob_hits.push((identity.entity_id, *position, target_id, skill));
    }

    for (attacker_id, attacker_pos, mob_id, skill) in buffer.pending_mob_hits.drain(..) {
        let Some(&mob_entity) = mob_index.map.get(&mob_id) else { continue };
        let Ok((mob_identity, mob_pos, mut mob_health, mut mob_state)) = mob_query.get_mut(mob_entity) else {
            continue;
        };
        if matches!(*mob_state, mobs::MobState::Dead { .. }) || mob_health.current <= 0 {
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

pub fn clear_unresolved_skills_system(mut query: Query<&mut CombatState>) {
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
    let last_used = combat_state.cooldowns.get(&skill_id).copied().unwrap_or(now - Duration::from_secs(60));
    now.duration_since(last_used) >= cooldown
}

fn update_skill_cooldown(
    skill_id: u32,
    combat_state: &mut CombatState,
    skill: SkillDef,
    now: Instant,
) {
    combat_state.cooldowns.insert(skill_id, now);
    combat_state.casting_until = now + skill.cast_time;
    combat_state.action = EntityAction::Casting;
}

fn peek_skill(combat_state: &CombatState) -> Option<(u32, u32)> {
    combat_state.pending_skill
}

fn try_consume_skill(
    combat_state: &mut CombatState,
    now: Instant,
) -> Option<(u32, u32, SkillDef)> {
    let (skill_id, target_id) = combat_state.pending_skill?;
    combat_state.pending_skill = None;

    let skill = skill_by_id(skill_id)?;
    if !skill_cooldown_elapsed(skill_id, combat_state, skill.cooldown, now) {
        return None;
    }
    update_skill_cooldown(skill_id, combat_state, skill, now);
    Some((skill_id, target_id, skill))
}

fn rewind_target_position(history: &PositionHistory, now: Instant) -> Position {
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

pub fn apply_mob_attacks_system(
    mut player_health_query: Query<(&NetworkIdentity, &mut Health), Without<Disconnected>>,
    entity_index: Res<EntityIndex>,
    mob_query: Query<(&mobs::MobIdentity, &mobs::MobState, &mobs::MobHealth, &crate::components::CombatStats)>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    let attack_window = Duration::from_millis(40);

    for (mob_identity, mob_state, mob_health, mob_stats) in mob_query.iter() {
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

        let damage = mob_stats.current_attack;
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
