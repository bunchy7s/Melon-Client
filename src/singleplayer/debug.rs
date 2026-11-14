//! The F3 entries vanilla can only fill with its integrated server
//! (`serverOrClientLevel instanceof ServerLevel`), read here off Pumpkin
//! running in Melon's own process: the local difficulty, the spawn counts,
//! the server's heightmaps, the climate at the feet and each chunk's state.

use std::fmt::Write as _;
use std::sync::atomic::Ordering;

use pumpkin::world::World;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkHeightmapType;

/// `DebugEntryLocalDifficulty`: `Local Difficulty: %.2f // %.2f`, inside
/// the build height only.
pub fn local_difficulty(world: &std::sync::Arc<World>, feet: [f64; 3]) -> Option<String> {
    let dimension = &world.dimension;
    let y = feet[1].floor() as i32;
    if y < dimension.min_y || y >= dimension.min_y + dimension.height {
        return None;
    }
    let difficulty = pumpkin::entity::mob::equipment::RegionalDifficulty::at(
        world,
        Vector3::new(feet[0], feet[1], feet[2]),
    );
    Some(format!(
        "Local Difficulty: {:.2} // {:.2}",
        difficulty.effective_difficulty, difficulty.special_multiplier
    ))
}

/// `MobCategory.getDebugAbbreviation`, in `MobCategory` order (Pumpkin's ids).
const CATEGORIES: [&str; 8] = ["MO", "C", "AM", "AX", "UWC", "WC", "WA", "MI"];

/// `DebugEntrySpawnCounts`: `SC: <chunks>, MO: n, C: n, ...` from the last
/// tick's spawn state.
pub fn spawn_counts(world: &World) -> Option<String> {
    let state = world.spawn_state.load();
    let (chunks, counts) = read_spawn_state(&**state)?;
    let mut line = format!("SC: {chunks}");
    for (name, count) in CATEGORIES.iter().zip(counts) {
        let _ = write!(line, ", {name}: {count}");
    }
    Some(line)
}

/// Pumpkin keeps the spawnable chunk count private and the counts behind
/// atomics; both are in its `Debug` output, first. Formatting stops right
/// after them, so the rest of the state (the potential and local caps,
/// large) is never written out.
fn read_spawn_state(state: &dyn std::fmt::Debug) -> Option<(i32, [i32; 8])> {
    struct UpTo {
        text: String,
    }
    impl std::fmt::Write for UpTo {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.text.push_str(s);
            // Past the counts' closing bracket: enough.
            let counts = self.text.find("mob_category_counts");
            match counts {
                Some(at) if self.text[at..].contains(']') => Err(std::fmt::Error),
                _ if self.text.len() > 4096 => Err(std::fmt::Error),
                _ => Ok(()),
            }
        }
    }
    let mut out = UpTo {
        text: String::new(),
    };
    let _ = write!(out, "{state:?}");
    parse_spawn_state(&out.text)
}

fn parse_spawn_state(text: &str) -> Option<(i32, [i32; 8])> {
    let after = |key: &str| text.find(key).map(|at| &text[at + key.len()..]);
    let chunks = after("spawnable_chunk_count:")?
        .trim_start()
        .split(|c: char| !c.is_ascii_digit() && c != '-')
        .next()?
        .parse()
        .ok()?;
    let list = after("mob_category_counts:")?;
    let list = &list[list.find('[')? + 1..];
    let list = &list[..list.find(']')?];
    let mut counts = [0; 8];
    let mut n = 0;
    for (slot, value) in counts.iter_mut().zip(list.split(',')) {
        *slot = value.trim().parse().ok()?;
        n += 1;
    }
    (n == 8).then_some((chunks, counts))
}

/// `DebugEntryHeightmap`'s server line: `SH S: .. O: .. M: .. ML: ..`.
/// Pumpkin keeps no ocean floor heightmap after generation: `O: ??`.
pub fn server_heightmaps(world: &World, x: i32, z: i32) -> String {
    let min_y = world.dimension.min_y;
    let heights = world
        .level
        .read_chunk_sync(&Vector2::new(x >> 4, z >> 4), |chunk| {
            let maps = chunk
                .heightmap
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // `get` gives the highest block; vanilla's height is the one above.
            let at = |kind| maps.get(kind, x, z, min_y) + 1;
            [
                at(ChunkHeightmapType::WorldSurface),
                at(ChunkHeightmapType::MotionBlocking),
                at(ChunkHeightmapType::MotionBlockingNoLeaves),
            ]
        });
    match heights {
        Some([s, m, ml]) => format!("SH S: {s} O: ?? M: {m} ML: {ml}"),
        None => "SH S: ?? O: ?? M: ?? ML: ??".to_owned(),
    }
}

/// `MultiNoiseBiomeSource.addDebugInfo`: the climate at the feet, in the
/// words of `OverworldBiomeBuilder`. Only a noise generator's multi noise
/// source has it (not the End's, not a flat world).
pub fn chunk_generation(world: &World, feet: [i32; 3]) -> Vec<String> {
    use pumpkin_world::generation::generator::WorldGenerator;
    use pumpkin_world::generation::noise::router::multi_noise_sampler::MultiNoiseSampler;
    if world.dimension.minecraft_name == "minecraft:the_end" {
        return Vec::new();
    }
    let generator = world.level.world_gen();
    let WorldGenerator::Noise(generator) = &*generator else {
        return Vec::new();
    };
    let mut sampler = MultiNoiseSampler::generate(&generator.base_router.multi_noise);
    let point = sampler.sample(feet[0] >> 2, feet[1] >> 2, feet[2] >> 2);
    let unquantize = |v: i64| v as f32 / 10000.0;
    vec![biome_builder_line(
        unquantize(point.continentalness),
        unquantize(point.erosion),
        unquantize(point.temperature),
        unquantize(point.humidity),
        unquantize(point.weirdness),
    )]
}

/// `TerrainProvider.peaksAndValleys`.
fn peaks_and_valleys(weirdness: f32) -> f32 {
    -((weirdness.abs() - 0.666_666_7).abs() - 0.333_333_34) * 3.0
}

/// `Climate.quantizeCoord`.
fn quantize(v: f32) -> i64 {
    (v * 10000.0) as i64
}

/// `OverworldBiomeBuilder.getDebugStringFor*`.
fn biome_builder_line(
    continentalness: f32,
    erosion: f32,
    temperature: f32,
    humidity: f32,
    weirdness: f32,
) -> String {
    let pv = peaks_and_valleys(weirdness);
    let pv = if pv < peaks_and_valleys(0.05) {
        "Valley"
    } else if pv < peaks_and_valleys(0.266_666_68) {
        "Low"
    } else if pv < peaks_and_valleys(0.4) {
        "Mid"
    } else if pv < peaks_and_valleys(0.566_666_66) {
        "High"
    } else {
        "Peak"
    };
    let c = quantize(continentalness);
    let continentalness = [
        (-1.05, "Mushroom fields"),
        (-0.455, "Deep ocean"),
        (-0.19, "Ocean"),
        (-0.11, "Coast"),
        (0.03, "Near inland"),
        (0.3, "Mid inland"),
    ]
    .iter()
    .find(|(max, _)| c < quantize(*max))
    .map_or("Far inland", |(_, name)| name);
    let index = |value: f32, maxima: &[f32]| {
        let q = quantize(value);
        maxima
            .iter()
            .position(|&max| q < quantize(max))
            .map_or_else(|| "?".to_owned(), |i| i.to_string())
    };
    format!(
        "Biome builder PV: {pv} C: {continentalness} E: {} T: {} H: {}",
        index(erosion, &[-0.78, -0.375, -0.2225, 0.05, 0.45, 0.55, 1.0]),
        index(temperature, &[-0.45, -0.15, 0.2, 0.55, 1.0]),
        index(humidity, &[-0.35, -0.1, 0.1, 0.3, 1.0]),
    )
}

/// `ServerChunkCache.getChunkDebugData`'s Pumpkin equivalent: whether the
/// chunk is loaded, and watched by a player (`W`), with its inhabited time.
pub fn chunk_state(world: &World, x: i32, z: i32) -> String {
    let pos = Vector2::new(x, z);
    let inhabited = world
        .level
        .read_chunk_sync(&pos, |c| c.inhabited_time.load(Ordering::Relaxed));
    match inhabited {
        None => "n/a".to_owned(),
        Some(ticks) => {
            let watched = if world.level.is_chunk_watched(&pos) {
                " W"
            } else {
                ""
            };
            format!("loaded{watched} {ticks}t")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spawn_state_is_read_from_its_debug_output() {
        let text = "SpawnState { spawnable_chunk_count: 289, mob_category_counts: [3, 1, 0, 0, 0, 2, 0, 5], spawn_potential: ";
        assert_eq!(
            parse_spawn_state(text),
            Some((289, [3, 1, 0, 0, 0, 2, 0, 5]))
        );
        assert_eq!(parse_spawn_state("SpawnState { }"), None);
    }

    #[test]
    fn formatting_stops_after_the_counts() {
        struct Huge;
        impl std::fmt::Debug for Huge {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(
                    "S { spawnable_chunk_count: 7, mob_category_counts: [1, 2, 3, 4, 5, 6, 7, 8]",
                )?;
                for _ in 0..100_000 {
                    f.write_str(", padding")?;
                }
                f.write_str(" }")
            }
        }
        assert_eq!(read_spawn_state(&Huge), Some((7, [1, 2, 3, 4, 5, 6, 7, 8])));
    }

    #[test]
    fn the_biome_builder_names_vanillas_bands() {
        assert_eq!(
            biome_builder_line(-1.1, -0.9, 0.0, 0.5, 0.0),
            "Biome builder PV: Valley C: Mushroom fields E: 0 T: 2 H: 4"
        );
        assert_eq!(
            biome_builder_line(0.5, 0.6, 0.9, -0.5, 0.666_666_7),
            "Biome builder PV: Peak C: Far inland E: 6 T: 4 H: 0"
        );
    }
}
