use bevy_ecs::prelude::*;
use std::time::Instant;
use shared::proto::{InputType, PlayerInput};
use crate::components::{CharacterId, CombatState, Disconnected, Health, LastActive, MovementIntent, NetworkIdentity, Position, PositionHistory, DodgeState, StatusEffects, CombatStats};
use crate::resources::{CharacterIdReceiver, EntityIndex, GlobalState, InputReceiver, NetworkInputBuffer, PendingCharacterAssignments};
use crate::constants::{MAX_NET_READ_BUDGET, PLAYER_DISCONNECT_GRACE, PLAYER_INACTIVITY_TIMEOUT};
use crate::experience;

pub fn process_network_inputs_system(
    mut input_rx: ResMut<InputReceiver>,
    mut global_state: ResMut<GlobalState>,
    mut entity_index: ResMut<EntityIndex>,
    mut buffer: ResMut<NetworkInputBuffer>,
    mut commands: Commands,
    mut query: Query<(&mut CombatState, &mut MovementIntent, &mut LastActive, Option<&Disconnected>)>,
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
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
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
                CombatState::new(Instant::now()),
                DodgeState::new(Instant::now()),
                PositionHistory::default(),
                MovementIntent::default(),
                experience::PlayerProgress::default(),
                LastActive { at: Instant::now() },
                StatusEffects::default(),
                CombatStats::default(),
            )).id()
        });

        let Ok((mut combat_state, mut intent, mut last_active, disconnected)) = query.get_mut(entity) else {
            continue;
        };

        if disconnected.is_some() {
            commands.entity(entity).remove::<Disconnected>();
            tracing::info!(
                entity_id = input.entity_id,
                "player reconnected — resuming session"
            );
        }

        last_active.at = Instant::now();

        // [Anti-Replay] Ignora pacotes antigos ou duplicados
        if input.sequence <= combat_state.last_processed_input {
            continue;
        }

        combat_state.last_processed_input = input.sequence;

        apply_input_to_intent(&input, &mut intent, &mut combat_state);
    }
}

pub fn apply_input_to_intent(
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

pub fn apply_character_assignments_system(
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
    loop {
        match char_rx.0.try_recv() {
            Ok(assignment) => {
                pending.pending.insert(assignment.entity_id, assignment);
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                tracing::warn!("character assignment channel disconnected");
                break;
            }
        }
    }

    let entity_ids: Vec<u32> = pending.pending.keys().copied().collect();

    for entity_id in entity_ids {
        let Some(&entity) = entity_index.map.get(&entity_id) else {
            tracing::debug!(entity_id, "character assignment buffered — entity not yet in ECS");
            continue;
        };

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

pub fn cleanup_disconnected_system(
    mut commands: Commands,
    query: Query<(Entity, &NetworkIdentity, &LastActive), Without<Disconnected>>,
) {
    let now = Instant::now();
    for (entity, identity, last_active) in query.iter() {
        if now.duration_since(last_active.at) > PLAYER_INACTIVITY_TIMEOUT {
            commands.entity(entity).insert(
                Disconnected { since: now, timeout: PLAYER_DISCONNECT_GRACE },
            );
            tracing::info!(
                entity_id = identity.entity_id,
                "player marked as disconnected — grace period started"
            );
        }
    }
}

pub fn cleanup_disconnected_timeout_system(
    mut commands: Commands,
    mut entity_index: ResMut<EntityIndex>,
    query: Query<(Entity, &NetworkIdentity, &Disconnected)>,
) {
    let now = Instant::now();
    for (entity, identity, disconnected) in query.iter() {
        if now.duration_since(disconnected.since) > disconnected.timeout {
            entity_index.map.remove(&identity.entity_id);
            commands.entity(entity).despawn();
            tracing::info!(
                entity_id = identity.entity_id,
                "player despawned — grace period expired"
            );
        }
    }
}
