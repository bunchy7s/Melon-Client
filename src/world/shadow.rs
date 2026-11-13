//! Vanilla entity blob shadows (`EntityRenderer.extractShadow` plus

use glam::Vec3;
use pumpkin_data::BlockStateId;

use crate::renderer::WorldVertex;
use crate::world::geometry;
use crate::world::level::Level;
use crate::world::lightmap;
use crate::world::physics::block_at;

/// `AvatarRenderer` / `LivingEntityRenderer` player shadow.
pub const PLAYER_RADIUS: f32 = 0.5;
/// `EntityRenderer.MAX_SHADOW_RADIUS`.
const MAX_RADIUS: f32 = 32.0;
/// `EntityRenderer`: `1 - distSq / 256`.
const DIST_FALLOFF: f32 = 256.0;
/// `SHADOW_POWER_FALLOFF_Y`.
const HEIGHT_FALLOFF: f32 = 0.5;
/// Raised off the block top so the depth test does not fight the terrain.
const Y_BIAS: f32 = 0.001;

/// Shadow quads under `feet` as seen from `camera`, vanilla's piece walk.
pub fn quads(level: &Level, feet: Vec3, camera: Vec3, radius: f32) -> Vec<WorldVertex> {
    let radius = radius.min(MAX_RADIUS);
    if radius <= 0.0 {
        return Vec::new();
    }
    let dist_sq = feet.distance_squared(camera);
    let pow = (1.0 - dist_sq / DIST_FALLOFF) * 1.0;
    if pow <= 0.0 {
        return Vec::new();
    }
    let depth = (pow / HEIGHT_FALLOFF - 1.0).min(radius);
    let x0 = (feet.x - radius).floor() as i32;
    let x1 = (feet.x + radius).floor() as i32;
    let z0 = (feet.z - radius).floor() as i32;
    let z1 = (feet.z + radius).floor() as i32;
    let y0 = (feet.y - depth).floor() as i32;
    let y1 = feet.y.floor() as i32;

    let mut out = Vec::new();
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                piece(&mut out, level, feet, pow, radius, [x, y, z]);
            }
        }
    }
    out
}

fn piece(
    out: &mut Vec<WorldVertex>,
    level: &Level,
    feet: Vec3,
    pow: f32,
    radius: f32,
    [x, y, z]: [i32; 3],
) {
    let below = BlockStateId::new(block_at(level, x, y - 1, z));
    let Some(state) = below.filter(|id| id.as_u16() != 0) else {
        return;
    };
    if !state.to_state().is_full_cube() {
        return;
    }
    let (block, sky) = level.light_at(x, y, z);
    let brightness = block.max(sky);
    if brightness <= 3 {
        return;
    }
    let light = lightmap::get_brightness(brightness);
    let power_at_depth = pow - (feet.y - y as f32) * HEIGHT_FALLOFF;
    let alpha = (power_at_depth * 0.5 * light).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let y_top = y as f32 + Y_BIAS;
    let x0 = x as f32;
    let x1 = x0 + 1.0;
    let z0 = z as f32;
    let z1 = z0 + 1.0;
    let u = |world_x: f32| -(world_x - feet.x) / 2.0 / radius + 0.5;
    let v = |world_z: f32| -(world_z - feet.z) / 2.0 / radius + 0.5;
    geometry::push_quad(
        out,
        [
            ([x0, y_top, z0], [u(x0), v(z0)]),
            ([x0, y_top, z1], [u(x0), v(z1)]),
            ([x1, y_top, z1], [u(x1), v(z1)]),
            ([x1, y_top, z0], [u(x1), v(z0)]),
        ],
        [1.0; 3],
        true,
    );
    let start = out.len().saturating_sub(4);
    for vertex in &mut out[start..] {
        vertex.color[3] = (alpha * 255.0).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::{Chunk, ChunkLight, Section};

    fn flat_world(sky: u8) -> Level {
        let mut level = Level::default();
        let mut sections: Vec<Section> = (0..24).map(|_| Section::empty()).collect();
        for section in &mut sections[..8] {
            *section = Section::filled(1);
        }
        level.insert(Chunk::new(0, 0, sections).with_light(ChunkLight::filled_sky(sky)));
        level
    }

    #[test]
    fn standing_player_casts_a_shadow() {
        let level = flat_world(15);
        let feet = Vec3::new(8.0, 64.0, 8.0);
        let camera = feet + Vec3::new(0.0, 1.62, 4.0);
        let verts = quads(&level, feet, camera, PLAYER_RADIUS);
        assert!(!verts.is_empty(), "expected shadow quads on the floor");
        assert!(verts.len().is_multiple_of(4));
    }

    #[test]
    fn unlit_ground_casts_no_shadow() {
        let level = flat_world(0);
        let feet = Vec3::new(8.0, 64.0, 8.0);
        let verts = quads(&level, feet, feet + Vec3::Z * 4.0, PLAYER_RADIUS);
        assert!(verts.is_empty());
    }
}
