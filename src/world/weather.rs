//! Rain and snow columns (vanilla `WeatherEffectRenderer`). Each column's

use std::sync::OnceLock;

use crate::renderer::WorldVertex;
use crate::world::java_random::JavaRandom;

const TABLE: usize = 32;
const HALF: i32 = 16;

/// Rain or snow at one XZ column.
#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub x: i32,
    pub z: i32,
    pub bottom_y: i32,
    pub top_y: i32,
    pub u_offset: f32,
    pub v_offset: f32,
    pub block_light: u8,
    pub sky_light: u8,
    pub snow: bool,
}

/// What [`columns`] needs of the world.
#[derive(Clone, Copy)]
pub struct WeatherQuery<'a> {
    pub intensity: f32,
    pub radius: i32,
    pub camera: glam::Vec3,
    pub game_time: i64,
    pub partial_tick: f32,
    pub height: &'a dyn Fn(i32, i32) -> i32,
    pub precipitation: &'a dyn Fn(i32, i32, i32) -> Precipitation,
    pub light: &'a dyn Fn(i32, i32, i32) -> (u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// Last `RAIN_LEVEL_CHANGE` / `THUNDER_LEVEL_CHANGE` the client applied.
#[derive(Clone, Copy, Debug, Default)]
pub struct WeatherLevels {
    pub rain: f32,
    pub thunder: f32,
}

impl WeatherLevels {
    pub fn set_rain(&mut self, level: f32) {
        self.rain = level.clamp(0.0, 1.0);
    }

    pub fn set_thunder(&mut self, level: f32) {
        self.thunder = level.clamp(0.0, 1.0);
    }
}

/// A column's part that holds while the camera stays in its block and the
#[derive(Clone, Copy, Debug)]
pub struct Prepared {
    x: i32,
    z: i32,
    bottom_y: i32,
    top_y: i32,
    fall: Fall,
    block_light: u8,
    sky_light: u8,
}

#[derive(Clone, Copy, Debug)]
enum Fall {
    Rain {
        tick_offset: i32,
        speed: f32,
    },
    /// `u = a + time * 0.01 * b`, `v = c + time * d * 0.001`.
    Snow {
        a: f32,
        b: f32,
        c: f32,
        d: f32,
    },
}

impl Prepared {
    /// This column's offsets at `game_time` and `partial_tick`.
    pub fn column(&self, game_time: i64, partial_tick: f32) -> Column {
        let ticks = game_time & 131_071;
        let (u_offset, v_offset, snow) = match self.fall {
            Fall::Rain { tick_offset, speed } => {
                let texture_offset =
                    -(ticks as f32 + tick_offset as f32 + partial_tick) / 32.0 * speed;
                (0.0, texture_offset % 32.0, false)
            }
            Fall::Snow { a, b, c, d } => {
                let time = ticks as f32 + partial_tick;
                let u = a + time * 0.01 * b;
                let v = c + time * d * 0.001;
                let v_offset = -((game_time & 511) as f32 + partial_tick) / 512.0;
                (u, v_offset + v, true)
            }
        };
        Column {
            x: self.x,
            z: self.z,
            bottom_y: self.bottom_y,
            top_y: self.top_y,
            u_offset,
            v_offset,
            block_light: self.block_light,
            sky_light: self.sky_light,
            snow,
        }
    }
}

/// What [`prepare`]'s result stands for: the camera's block, the world's
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrepareKey {
    pub block: [i32; 3],
    pub revision: u64,
    pub radius: i32,
}

/// The world's side of the columns around the camera.
pub fn prepare(query: &WeatherQuery<'_>) -> Vec<Prepared> {
    if query.radius <= 0 {
        return Vec::new();
    }
    let cam_x = query.camera.x.floor() as i32;
    let cam_y = query.camera.y.floor() as i32;
    let cam_z = query.camera.z.floor() as i32;
    let r = query.radius;
    let mut out = Vec::new();
    for z in (cam_z - r)..=(cam_z + r) {
        for x in (cam_x - r)..=(cam_x + r) {
            let terrain = (query.height)(x, z);
            let y0 = (cam_y - r).max(terrain);
            let y1 = (cam_y + r).max(terrain);
            if y1 == y0 {
                continue;
            }
            let kind = (query.precipitation)(x, cam_y, z);
            if kind == Precipitation::None {
                continue;
            }
            let mut rng = JavaRandom::with_seed(column_seed(x, z));
            let light_y = cam_y.max(terrain);
            let (block, sky) = (query.light)(x, light_y, z);
            let (fall, block, sky) = match kind {
                Precipitation::Rain => (
                    Fall::Rain {
                        tick_offset: rain_tick_offset(x, z),
                        speed: 3.0 + rng.next_float(),
                    },
                    block,
                    sky,
                ),
                Precipitation::Snow => {
                    let a = rng.next_double() as f32;
                    let b = rng.next_gaussian() as f32;
                    let c = rng.next_double() as f32;
                    let d = rng.next_gaussian() as f32;
                    let block = ((i32::from(block) * 3 + 15) / 4).clamp(0, 15) as u8;
                    let sky = ((i32::from(sky) * 3 + 15) / 4).clamp(0, 15) as u8;
                    (Fall::Snow { a, b, c, d }, block, sky)
                }
                Precipitation::None => continue,
            };
            out.push(Prepared {
                x,
                z,
                bottom_y: y0,
                top_y: y1,
                fall,
                block_light: block,
                sky_light: sky,
            });
        }
    }
    out
}

/// Seeded columns around the camera.
pub fn columns(query: WeatherQuery<'_>) -> Vec<Column> {
    if query.intensity <= 0.0 {
        return Vec::new();
    }
    prepare(&query)
        .iter()
        .map(|p| p.column(query.game_time, query.partial_tick))
        .collect()
}

/// `x * x * 3121 + x * 45238971 ^ z * z * 418711 + z * 13761` as Java ints.
pub fn column_seed(x: i32, z: i32) -> i64 {
    let left = x
        .wrapping_mul(x)
        .wrapping_mul(3121)
        .wrapping_add(x.wrapping_mul(45_238_971));
    let right = z
        .wrapping_mul(z)
        .wrapping_mul(418_711)
        .wrapping_add(z.wrapping_mul(13_761));
    i64::from(left ^ right)
}

fn rain_tick_offset(x: i32, z: i32) -> i32 {
    x.wrapping_mul(x)
        .wrapping_mul(3121)
        .wrapping_add(x.wrapping_mul(45_238_971))
        .wrapping_add(z.wrapping_mul(z).wrapping_mul(418_711))
        .wrapping_add(z.wrapping_mul(13_761))
        & 0xFF
}

/// Camera-relative quads. Rain first, then snow, matching vanilla's two draws.
pub fn mesh(
    columns: &[Column],
    camera: glam::Vec3,
    radius: i32,
    intensity: f32,
    lightmap: &crate::world::lightmap::LightmapParams,
) -> (Vec<WorldVertex>, Vec<WorldVertex>) {
    let mut rain = Vec::new();
    let mut snow = Vec::new();
    if radius <= 0 {
        return (rain, snow);
    }
    let radius_sq = (radius * radius) as f32;
    let cam_x = camera.x.floor() as i32;
    let cam_z = camera.z.floor() as i32;
    let sizes = column_sizes();
    for column in columns {
        let relative_x = column.x as f32 + 0.5 - camera.x;
        let relative_z = column.z as f32 + 0.5 - camera.z;
        let dist_sq = relative_x * relative_x + relative_z * relative_z;
        let max_alpha = if column.snow { 0.8 } else { 1.0 };
        let t = (dist_sq / radius_sq).min(1.0);
        let alpha = (max_alpha + (0.5 - max_alpha) * t) * intensity;
        let idx = ((column.z - cam_z + HALF) * TABLE as i32 + (column.x - cam_x + HALF)) as usize;
        let (half_x, half_z) = sizes.get(idx).copied().unwrap_or((0.5, 0.0));
        let x0 = relative_x - half_x;
        let x1 = relative_x + half_x;
        let z0 = relative_z - half_z;
        let z1 = relative_z + half_z;
        let y1 = column.top_y as f32 - camera.y;
        let y0 = column.bottom_y as f32 - camera.y;
        let u0 = column.u_offset;
        let u1 = column.u_offset + 1.0;
        let v0 = column.bottom_y as f32 * 0.25 + column.v_offset;
        let v1 = column.top_y as f32 * 0.25 + column.v_offset;
        let [lr, lg, lb] =
            crate::world::lightmap::sample(column.block_light, column.sky_light, lightmap);
        let color = [lr, lg, lb, alpha];
        let quad = [
            WorldVertex::rgba([x0, y1, z0], [u0, v0], color),
            WorldVertex::rgba([x1, y1, z1], [u1, v0], color),
            WorldVertex::rgba([x1, y0, z1], [u1, v1], color),
            WorldVertex::rgba([x0, y0, z0], [u0, v1], color),
        ];
        if column.snow {
            snow.extend(quad);
        } else {
            rain.extend(quad);
        }
    }
    (rain, snow)
}

fn column_sizes() -> &'static [(f32, f32); TABLE * TABLE] {
    static SIZES: OnceLock<[(f32, f32); TABLE * TABLE]> = OnceLock::new();
    SIZES.get_or_init(|| {
        let mut out = [(0.0, 0.0); TABLE * TABLE];
        for z in 0..TABLE {
            for x in 0..TABLE {
                let dx = x as f32 - 16.0;
                let dz = z as f32 - 16.0;
                let len = (dx * dx + dz * dz).sqrt().max(1e-6);
                out[z * TABLE + x] = ((-dz / len) * 0.5, (dx / len) * 0.5);
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_keeps_its_vanilla_seed() {
        let left = 3 * 3 * 3121 + 3 * 45_238_971;
        let right = 5 * 5 * 418_711 + 5 * 13_761;
        assert_eq!(column_seed(3, 5), i64::from(left ^ right));
    }

    #[test]
    fn the_same_query_rebuilds_the_same_columns() {
        let height = |_, _| 64;
        let precip = |_, _, _| Precipitation::Rain;
        let light = |_, _, _| (0u8, 15u8);
        let query = WeatherQuery {
            intensity: 1.0,
            radius: 2,
            camera: glam::Vec3::new(8.2, 70.0, 8.4),
            game_time: 2400,
            partial_tick: 0.25,
            height: &height,
            precipitation: &precip,
            light: &light,
        };
        let a = columns(query);
        let b = columns(WeatherQuery {
            height: &height,
            precipitation: &precip,
            light: &light,
            ..query
        });
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].v_offset, b[0].v_offset);
    }

    #[test]
    fn prepared_columns_move_with_time_only() {
        let height = |_, _| 64;
        let precip = |x, _, _| {
            if x % 2 == 0 {
                Precipitation::Rain
            } else {
                Precipitation::Snow
            }
        };
        let light = |_, _, _| (3u8, 12u8);
        let query = WeatherQuery {
            intensity: 1.0,
            radius: 3,
            camera: glam::Vec3::new(8.2, 70.0, 8.4),
            game_time: 0,
            partial_tick: 0.0,
            height: &height,
            precipitation: &precip,
            light: &light,
        };
        let prepared = prepare(&query);
        assert_eq!(prepared.len(), columns(query).len());
        for p in &prepared {
            let (a, b) = (p.column(100, 0.5), p.column(101, 0.5));
            assert_eq!(
                (a.x, a.z, a.bottom_y, a.top_y),
                (b.x, b.z, b.bottom_y, b.top_y)
            );
            assert_ne!(a.v_offset, b.v_offset);
        }
    }

    #[test]
    fn the_lightmap_goes_into_the_colour() {
        let column = Column {
            x: 11,
            z: 8,
            bottom_y: 60,
            top_y: 80,
            u_offset: 0.0,
            v_offset: 0.0,
            block_light: 0,
            sky_light: 0,
            snow: false,
        };
        let params = crate::world::lightmap::LightmapParams::overworld([1.0; 3], 0.0);
        let camera = glam::Vec3::new(8.2, 70.0, 8.4);
        let dark = mesh(&[column], camera, 8, 1.0, &params).0;
        let lit = mesh(
            &[Column {
                sky_light: 15,
                ..column
            }],
            camera,
            8,
            1.0,
            &params,
        )
        .0;
        assert_eq!(dark.len(), 4);
        assert!(lit[0].color[0] > dark[0].color[0]);
    }
}
