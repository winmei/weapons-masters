use bevy_ecs::prelude::*;
use std::time::Instant;
use shared::proto::{combat_event, CombatEvent, DodgeResult, EntityAction, Vec2};
use crate::components::{CombatState, Disconnected, DodgeState, Health, MovementIntent, NetworkIdentity, Position, CombatStats};
use crate::resources::CombatEventQueue;
use crate::spatial::{resolve_world_collisions, SpatialHash};
use crate::constants::{DODGE_COOLDOWN, DODGE_DISTANCE, DODGE_IFRAMES, TICK_DELTA};

pub fn apply_movement_and_dodge_system(
    mut query: Query<(
        &NetworkIdentity,
        &mut Position,
        &mut CombatState,
        &mut DodgeState,
        &mut MovementIntent,
        &CombatStats,
        &Health,
    ), Without<Disconnected>>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();
    for (identity, mut position, mut combat_state, mut dodge_state, mut intent, stats, health) in
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

        apply_movement_intent(&mut position, &mut combat_state, &intent, stats);
        resolve_world_collisions(&mut position, &mut combat_state);
    }
}

pub fn apply_movement_intent(
    position: &mut Position,
    combat_state: &mut CombatState,
    intent: &MovementIntent,
    stats: &CombatStats,
) {
    if let Some(direction) = intent.direction.as_ref() {
        let length = (direction.x * direction.x + direction.y * direction.y).sqrt();
        if length > f32::EPSILON {
            let step = stats.current_speed * TICK_DELTA;
            position.x += (direction.x / length) * step;
            position.y += (direction.y / length) * step;
            combat_state.rotation = direction.y.atan2(direction.x);
            combat_state.action = EntityAction::Moving;
            return;
        }
    }
    combat_state.action = EntityAction::Idle;
}

pub fn apply_dodge_intent(
    entity_id: u32,
    direction: Vec2,
    now: Instant,
    position: &mut Position,
    combat_state: &mut CombatState,
    dodge_state: &mut DodgeState,
    events: &mut CombatEventQueue,
) {
    if !dodge_state.can_start_at(now) {
        events.events.push(CombatEvent {
            event: Some(combat_event::Event::Dodge(DodgeResult { entity_id, success: false })),
        });
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

pub fn rebuild_spatial_hash_system(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &Position, &Health), Changed<Position>>,
) {
    for (entity, position, health) in query.iter() {
        if health.current > 0 {
            spatial_hash.update_entity(entity, *position);
        } else {
            spatial_hash.remove_entity(entity);
        }
    }
}
