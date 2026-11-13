//! How the renderer places a mob: its scale and offsets, the hurt

use super::*;

/// How the renderer places the model: what its kind fixes
pub fn placement(entity: &ClientEntity, partial: f32) -> Placement {
    let look = entity.kind.look();
    let adult = !entity.appearance.baby;
    let mut placement = Placement {
        scale: Vec3::splat(if adult || !look.adult_scale {
            look.scale
        } else {
            1.0
        }),
        flip_degrees: look.flip_degrees,
        ..Default::default()
    };
    match entity.kind {
        EntityKind::Creeper => placement.scale = creeper::swell_scale(entity.swelling(partial)),
        EntityKind::Cat => placement.lie_down = entity.feline_amounts(partial).lie_down,
        EntityKind::Slime | EntityKind::MagmaCube => {
            let size = entity.appearance.size as f32;
            let squish = entity.squish(partial);
            let w = 1.0 / (squish / (size * 0.5 + 1.0) + 1.0);
            let hair = if entity.kind == EntityKind::Slime {
                0.999
            } else {
                1.0
            };
            placement.scale = Vec3::new(w * size, size / w, w * size) * hair;
        }
        EntityKind::Cod => {
            placement.fish = Some(fish::Wobble::cod(
                entity.age_in_ticks(partial),
                entity.in_water,
            ));
        }
        EntityKind::Salmon => {
            placement.fish = Some(fish::Wobble::salmon(
                entity.age_in_ticks(partial),
                entity.in_water,
            ));
        }
        EntityKind::TropicalFish => {
            placement.fish = Some(fish::Wobble::tropical(
                entity.age_in_ticks(partial),
                entity.in_water,
            ));
        }
        EntityKind::Pufferfish => {
            placement.fish = Some(fish::Wobble::pufferfish(entity.age_in_ticks(partial)));
        }
        EntityKind::Squid | EntityKind::GlowSquid => {
            placement.squid = Some(entity.squid_pose(partial).1);
        }
        EntityKind::IronGolem => {
            placement.roll_degrees =
                golem::roll(entity.walk.position(partial), entity.walk.speed(partial));
        }
        EntityKind::Fox
            if entity.appearance.fox_flags & (fox::FLAG_POUNCING | fox::FLAG_FACEPLANTED) != 0 =>
        {
            placement.extra_pitch = -entity.pitch_degrees(partial);
        }
        EntityKind::Phantom => {
            placement.scale *= phantom::size_scale(entity.appearance.size);
        }
        _ => {}
    }
    placement
}

/// How far a hurt body is mixed toward red: `OverlayTexture`'s red row has
pub const HURT_OVERLAY: f32 = 1.0 - 178.0 / 255.0;

/// `hasRedOverlay`: the overlay of a body that is hurt or dying. The one
pub const fn hurt_overlay(hurt: bool) -> f32 {
    if hurt { HURT_OVERLAY } else { 0.0 }
}

/// The overlay this frame: hurt red wins over a creeper's white flash.
pub fn overlay(entity: &ClientEntity, state: &LivingRenderState, partial: f32) -> f32 {
    if state.hurt {
        return hurt_overlay(true);
    }
    if entity.kind == EntityKind::Creeper {
        return -creeper::white_flash(entity.swelling(partial));
    }
    0.0
}

/// `GlowSquidRenderer.getBlockLightLevel`: the light it gives off, 15
pub fn block_glow(entity: &ClientEntity) -> u8 {
    match entity.kind {
        EntityKind::MagmaCube | EntityKind::Blaze => 15,
        EntityKind::GlowSquid => {
            let lit = 1.0 - entity.appearance.dark_ticks as f32 / 10.0;
            (lit.clamp(0.0, 1.0) * 15.0) as u8
        }
        _ => 0,
    }
}

/// The renderer's `shadowRadius`, halved for babies (`MobRenderer`).
pub fn shadow_radius(entity: &ClientEntity) -> f32 {
    let radius = match entity.kind {
        EntityKind::Slime | EntityKind::MagmaCube => 0.25 * entity.appearance.size as f32,
        EntityKind::Pufferfish => 0.1 + 0.1 * entity.appearance.puff_state as f32,
        kind => kind.look().shadow,
    };
    if entity.appearance.baby {
        radius * 0.5
    } else {
        radius
    }
}
