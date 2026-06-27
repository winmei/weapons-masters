use bevy_ecs::prelude::*;
use std::time::Instant;
use crate::components::{CombatStats, Health, StatusEffects, EffectType, StatType, NetworkIdentity};
use shared::proto::{combat_event, CombatEvent, DamageEvent};
use crate::resources::CombatEventQueue;

pub fn update_status_effects_system(
    mut query: Query<(&NetworkIdentity, &mut StatusEffects, &mut CombatStats, &mut Health)>,
    mut events: ResMut<CombatEventQueue>,
) {
    let now = Instant::now();

    for (identity, mut status, mut stats, mut health) in query.iter_mut() {
        if health.current <= 0 {
            status.active.clear();
            continue;
        }

        let mut needs_stat_recalc = false;

        // Processa os efeitos ativos
        for effect in &mut status.active {
            if now >= effect.expires_at {
                needs_stat_recalc = true;
                continue;
            }

            match &mut effect.effect_type {
                EffectType::DoT { damage_per_tick, tick_interval, last_tick } => {
                    if now.duration_since(*last_tick) >= *tick_interval {
                        *last_tick = now;
                        health.current = health.current.saturating_sub(*damage_per_tick).max(0);
                        
                        // Emitir evento de dano para o cliente ver o número saltar
                        events.events.push(CombatEvent {
                            event: Some(combat_event::Event::Damage(DamageEvent {
                                target_entity_id: identity.entity_id,
                                source_entity_id: effect.source_entity.unwrap_or(0),
                                damage: *damage_per_tick,
                                remaining_hp: health.current,
                            })),
                        });
                    }
                }
                _ => {}
            }
        }

        // Limpa efeitos expirados
        let original_len = status.active.len();
        status.active.retain(|e| now < e.expires_at);
        if status.active.len() != original_len {
            needs_stat_recalc = true;
        }

        // Recalcula atributos se algum modificador foi adicionado/removido
        if needs_stat_recalc {
            let mut atk_flat = 0.0;
            let mut atk_pct = 0.0;
            let mut def_flat = 0.0;
            let mut def_pct = 0.0;
            let mut spd_flat = 0.0;
            let mut spd_pct = 0.0;

            for effect in &status.active {
                if let EffectType::StatModifier { stat, value, is_percentage } = &effect.effect_type {
                    match stat {
                        StatType::Attack => if *is_percentage { atk_pct += value } else { atk_flat += value },
                        StatType::Defense => if *is_percentage { def_pct += value } else { def_flat += value },
                        StatType::Speed => if *is_percentage { spd_pct += value } else { spd_flat += value },
                    }
                }
            }

            stats.current_attack = ((stats.base_attack as f32 * (1.0 + atk_pct)) + atk_flat) as i32;
            stats.current_defense = ((stats.base_defense as f32 * (1.0 + def_pct)) + def_flat) as i32;
            stats.current_speed = (stats.base_speed * (1.0 + spd_pct)) + spd_flat;
        }
    }
}
