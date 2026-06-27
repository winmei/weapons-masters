//! Sistema de progressão de jogadores — Step 3
//! XP por kill de mob → level up → scale de stats

use bevy_ecs::prelude::*;
use crate::mobs::{RewardEventQueue, XpEvent};

/// Componente: nível e experiência do jogador
#[derive(Component, Clone, Debug, Default)]
pub struct PlayerProgress {
    pub level: u32,
    pub experience: u64,
}

impl PlayerProgress {
    /// XP necessário para ir do nível N para N+1 = N * 100
    pub fn xp_for_level(level: u32) -> u64 {
        (level as u64) * 100
    }

    /// Adiciona XP e sobe de nível se necessário.
    /// Retorna o número de níveis ganhos (pode ser > 1 com XP elevado).
    pub fn add_xp(&mut self, xp: u32) -> u32 {
        self.experience += xp as u64;
        let mut levels_gained = 0u32;
        loop {
            let threshold = Self::xp_for_level(self.level.max(1));
            if self.experience >= threshold && self.level < 100 {
                self.experience -= threshold;
                self.level += 1;
                levels_gained += 1;
            } else {
                break;
            }
        }
        levels_gained
    }
}

/// Eventos de level-up para enviar ao cliente e ao persistence
#[derive(Resource, Default)]
pub struct LevelUpEventQueue {
    pub events: Vec<LevelUpNotification>,
}

#[derive(Clone, Debug)]
pub struct LevelUpNotification {
    pub entity_id: u32,
    pub new_level: u32,
    pub new_experience: u64,
}

/// Sistema que processa a fila de XP e aplica ao componente PlayerProgress
pub fn experience_system(
    mut reward_queue: ResMut<RewardEventQueue>,
    mut levelup_queue: ResMut<LevelUpEventQueue>,
    entity_index: Res<crate::EntityIndex>,
    mut query: Query<(&crate::NetworkIdentity, &mut PlayerProgress)>,
) {
    levelup_queue.events.clear();

    let xp_events: Vec<XpEvent> = reward_queue.xp_events.drain(..).collect();

    for xp_event in xp_events {
        let Some(entity) = entity_index.map.get(&xp_event.entity_id).copied() else {
            continue;
        };
        let Ok((identity, mut progress)) = query.get_mut(entity) else {
            continue;
        };

        let levels_gained = progress.add_xp(xp_event.xp);
        tracing::debug!(
            entity_id = identity.entity_id,
            xp = xp_event.xp,
            new_level = progress.level,
            new_exp = progress.experience,
            levels_gained,
            "XP added"
        );

        if levels_gained > 0 {
            tracing::info!(
                entity_id = identity.entity_id,
                new_level = progress.level,
                levels_gained,
                "LEVEL UP"
            );
            levelup_queue.events.push(LevelUpNotification {
                entity_id: identity.entity_id,
                new_level: progress.level,
                new_experience: progress.experience,
            });
        }
    }
}
