use bevy_ecs::prelude::*;
use shared::proto::{InputType, PlayerInput};
use std::time::Instant;

use crate::components::{
    CharacterId, CombatState, CombatStats, Disconnected, DodgeState, Health, LastActive,
    MovementIntent, NetworkIdentity, Position, PositionHistory, StatusEffects, WorldMapId,
};
use crate::constants::{MAX_NET_READ_BUDGET, PLAYER_DISCONNECT_GRACE, PLAYER_INACTIVITY_TIMEOUT};
use crate::experience;
use crate::resources::{
    CharacterEntityIndex, EnterWorldCommand, EnterWorldReceiver, EntityIndex, GlobalState,
    InputReceiver, NetworkInputBuffer,
};

pub fn process_network_inputs_system(
    mut input_rx: ResMut<InputReceiver>,
    mut global_state: ResMut<GlobalState>,
    entity_index: Res<EntityIndex>,
    mut buffer: ResMut<NetworkInputBuffer>,
    mut query: Query<(
        &mut CombatState,
        &mut MovementIntent,
        &mut LastActive,
        Option<&Disconnected>,
    )>,
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
        let Some(&entity) = entity_index.map.get(&input.entity_id) else {
            tracing::warn!(
                entity_id = input.entity_id,
                "input received for an entity that has not entered the world"
            );
            continue;
        };

        let Ok((mut combat_state, mut intent, mut last_active, disconnected)) =
            query.get_mut(entity)
        else {
            continue;
        };

        if disconnected.is_some() {
            tracing::warn!(
                entity_id = input.entity_id,
                "input ignored for disconnected entity; authentication must re-enter the world"
            );
            continue;
        }

        global_state.last_processed_input = global_state.last_processed_input.max(input.sequence);
        last_active.at = Instant::now();

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

/// Drains authenticated entry commands and mutates the ECS immediately. This
/// exclusive system makes entry idempotent even when duplicate commands arrive
/// in the same tick.
pub fn enter_world_system(world: &mut World) {
    let mut commands = Vec::new();
    {
        let mut receiver = world.resource_mut::<EnterWorldReceiver>();
        loop {
            match receiver.0.try_recv() {
                Ok(command) => commands.push(command),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                    tracing::warn!("enter-world channel disconnected");
                    break;
                }
            }
        }
    }

    for command in commands {
        apply_enter_world_command(world, command);
    }
}

pub fn apply_enter_world_command(world: &mut World, command: EnterWorldCommand) -> bool {
    if command.entity_id == 0 || command.character_id <= 0 {
        tracing::warn!(
            entity_id = command.entity_id,
            character_id = command.character_id,
            "world entry rejected: invalid authoritative identity"
        );
        return false;
    }

    let entity_for_id = world
        .resource::<EntityIndex>()
        .map
        .get(&command.entity_id)
        .copied();
    let entity_for_character = world
        .resource::<CharacterEntityIndex>()
        .map
        .get(&command.character_id)
        .copied();

    if let (Some(by_id), Some(by_character)) = (entity_for_id, entity_for_character) {
        if by_id != by_character {
            tracing::warn!(
                entity_id = command.entity_id,
                character_id = command.character_id,
                "world entry rejected: entity id and character belong to different entities"
            );
            return false;
        }
    }

    if let Some(entity) = entity_for_id.or(entity_for_character) {
        let Some(existing_character_id) = world.get::<CharacterId>(entity).map(|id| id.value)
        else {
            tracing::warn!(
                entity_id = command.entity_id,
                character_id = command.character_id,
                "world entry rejected: existing entity has no character identity"
            );
            return false;
        };

        if existing_character_id != 0 && existing_character_id != command.character_id {
            tracing::warn!(
                entity_id = command.entity_id,
                existing_character_id,
                requested_character_id = command.character_id,
                "world entry rejected: entity is already owned by another character"
            );
            return false;
        }

        let Some(previous_entity_id) = world
            .get::<NetworkIdentity>(entity)
            .map(|identity| identity.entity_id)
        else {
            tracing::warn!(
                entity_id = command.entity_id,
                "world entry rejected: existing entity has no network identity"
            );
            return false;
        };

        let has_required_components = world.get::<WorldMapId>(entity).is_some()
            && world.get::<Position>(entity).is_some()
            && world.get::<Health>(entity).is_some()
            && world.get::<experience::PlayerProgress>(entity).is_some()
            && world.get::<CombatState>(entity).is_some()
            && world.get::<LastActive>(entity).is_some();
        if !has_required_components {
            tracing::warn!(
                entity_id = command.entity_id,
                character_id = command.character_id,
                "world entry rejected: existing entity has an incompatible component set"
            );
            return false;
        }

        if let Some(mut identity) = world.get_mut::<NetworkIdentity>(entity) {
            identity.entity_id = command.entity_id;
        }
        if let Some(mut character_id) = world.get_mut::<CharacterId>(entity) {
            character_id.value = command.character_id;
        }
        if let Some(mut map_id) = world.get_mut::<WorldMapId>(entity) {
            map_id.value = command.map_id.clone();
        }
        if let Some(mut position) = world.get_mut::<Position>(entity) {
            position.x = command.position_x;
            position.y = command.position_y;
        }
        if let Some(mut health) = world.get_mut::<Health>(entity) {
            health.current = command.current_hp;
            health.max = command.maximum_hp;
        }
        if let Some(mut progress) = world.get_mut::<experience::PlayerProgress>(entity) {
            progress.level = command.level;
            progress.experience = command.experience;
        }
        if let Some(mut combat_state) = world.get_mut::<CombatState>(entity) {
            combat_state.rotation = command.rotation;
        }
        if let Some(mut last_active) = world.get_mut::<LastActive>(entity) {
            last_active.at = Instant::now();
        }
        world.entity_mut(entity).remove::<Disconnected>();

        {
            let mut entity_index = world.resource_mut::<EntityIndex>();
            entity_index.map.remove(&previous_entity_id);
            entity_index.map.insert(command.entity_id, entity);
        }
        world
            .resource_mut::<CharacterEntityIndex>()
            .map
            .insert(command.character_id, entity);

        tracing::info!(
            entity_id = command.entity_id,
            character_id = command.character_id,
            "authenticated character re-entered the world using its existing ECS entity"
        );
        return true;
    }

    let now = Instant::now();
    let entity = world
        .spawn((
            NetworkIdentity {
                entity_id: command.entity_id,
            },
            CharacterId {
                value: command.character_id,
            },
            WorldMapId {
                value: command.map_id,
            },
            Position {
                x: command.position_x,
                y: command.position_y,
            },
            Health {
                current: command.current_hp,
                max: command.maximum_hp,
            },
            CombatState {
                rotation: command.rotation,
                ..CombatState::new(now)
            },
            DodgeState::new(now),
            PositionHistory::default(),
            MovementIntent::default(),
            experience::PlayerProgress {
                level: command.level,
                experience: command.experience,
            },
            LastActive { at: now },
            StatusEffects::default(),
            CombatStats::default(),
        ))
        .id();

    world
        .resource_mut::<EntityIndex>()
        .map
        .insert(command.entity_id, entity);
    world
        .resource_mut::<CharacterEntityIndex>()
        .map
        .insert(command.character_id, entity);

    tracing::info!(
        entity_id = command.entity_id,
        character_id = command.character_id,
        "authenticated character entered the world"
    );
    true
}

pub fn cleanup_disconnected_system(
    mut commands: Commands,
    query: Query<(Entity, &NetworkIdentity, &LastActive), Without<Disconnected>>,
) {
    let now = Instant::now();
    for (entity, identity, last_active) in query.iter() {
        if now.duration_since(last_active.at) > PLAYER_INACTIVITY_TIMEOUT {
            commands.entity(entity).insert(Disconnected {
                since: now,
                timeout: PLAYER_DISCONNECT_GRACE,
            });
            tracing::info!(
                entity_id = identity.entity_id,
                "player marked as disconnected; grace period started"
            );
        }
    }
}

pub fn cleanup_disconnected_timeout_system(
    mut commands: Commands,
    mut entity_index: ResMut<EntityIndex>,
    mut character_index: ResMut<CharacterEntityIndex>,
    query: Query<(Entity, &NetworkIdentity, &CharacterId, &Disconnected)>,
) {
    let now = Instant::now();
    for (entity, identity, character_id, disconnected) in query.iter() {
        if now.duration_since(disconnected.since) > disconnected.timeout {
            entity_index.map.remove(&identity.entity_id);
            character_index.map.remove(&character_id.value);
            commands.entity(entity).despawn();
            tracing::info!(
                entity_id = identity.entity_id,
                "player despawned; grace period expired"
            );
        }
    }
}
