//! Vanilla's smooth lighting and ambient occlusion for block faces.
//!
//! [`LightCache`] flat 20³ array: one cell per block of the
//! section and a two-block shell. Light layers are unpacked with the byte
//! kernels ([`crate::world::nibble_decode`]) and written in once a section,
//! so the quad loop only indexes.

use crate::renderer::CornerLight;
use crate::world::block::ao_flags::{self, AoFlags};
use crate::world::block::direction::Direction;
use crate::world::block_models::ao_shape::AoShape;
use crate::world::chunk::LightSection;
use crate::world::nibble_decode;
use crate::world::section::{BLOCKS_PER_SECTION, Section};
use crate::world::terrain::Neighborhood;

/// `LightCoordsUtil.pack`.
fn pack(block: u32, sky: u32) -> u32 {
    block << 4 | sky << 20
}

/// `LightCoordsUtil.block` / `sky` of packed coordinates.
fn block_of(packed: u32) -> u32 {
    packed >> 4 & 15
}
fn sky_of(packed: u32) -> u32 {
    packed >> 20 & 15
}

/// `LightCoordsUtil.FULL_BRIGHT`, what `emissiveRendering` blocks get.
const FULL_BRIGHT: u32 = 0xF0_00F0;

/// `LightCoordsUtil.smoothBlend`: three neighbours and the centre averaged;
fn smooth_blend(mut n1: u32, mut n2: u32, mut n3: u32, center: u32) -> u32 {
    if sky_of(center) > 2 || block_of(center) > 2 {
        for n in [&mut n1, &mut n2, &mut n3] {
            if *n == 0 {
                *n = center;
            } else if sky_of(*n) == 0 {
                *n |= center & 0xFF_0000;
            }
        }
    }
    (n1 + n2 + n3 + center) >> 2 & 0xFF_00FF
}

/// Java `(int)float` toward zero, then `& 0xFF` as `smoothPack` does.
fn java_u8(v: f32) -> u32 {
    (v as i32 as u32) & 0xFF
}

/// `LightCoordsUtil.smoothWeightedBlend` of four smooth coordinates.
fn smooth_weighted_blend(coords: [u32; 4], weights: [f32; 4]) -> u32 {
    let mut sky = 0.0f32;
    let mut block = 0.0f32;
    for (c, w) in coords.into_iter().zip(weights) {
        sky += (c >> 16 & 0xFF) as f32 * w;
        block += (c & 0xFF) as f32 * w;
    }
    java_u8(block) | java_u8(sky) << 16
}

/// One cell of the cache: its light coordinates as vanilla's
#[derive(Clone, Copy)]
struct Cell {
    light: u32,
    flags: AoFlags,
    block: u8,
    sky: u8,
}

impl Cell {
    const EMPTY: Self = Self {
        light: 0,
        flags: AoFlags::AIR,
        block: 0,
        sky: 0,
    };

    /// `getShadeBrightness`.
    fn shade(self) -> f32 {
        if self.flags.shade_dark() { 0.2 } else { 1.0 }
    }
}

/// How far around the section faces look: a face whose light comes from
const SHELL: i32 = 2;
/// Side of the cached cube: a section plus the shell on every side.
const SIDE: usize = 16 + 2 * SHELL as usize;
const CELLS: usize = SIDE * SIDE * SIDE;

/// Index of section-relative `(x, y, z)` in `-2..=17`.
#[inline]
fn cell_index(x: i32, y: i32, z: i32) -> usize {
    debug_assert!(
        (-SHELL..16 + SHELL).contains(&x)
            && (-SHELL..16 + SHELL).contains(&y)
            && (-SHELL..16 + SHELL).contains(&z),
        "{x},{y},{z} outside the cache"
    );
    ((y + SHELL) as usize * SIDE + (z + SHELL) as usize) * SIDE + (x + SHELL) as usize
}

/// The light and flags of a section's blocks and their neighbours, filled
/// densely once a section (Sodium's `ArrayLightDataCache` layout, the
/// layers unpacked with miraculix).
pub struct LightCache {
    cells: Vec<Cell>,
    section: usize,
}

impl Default for LightCache {
    fn default() -> Self {
        Self {
            cells: vec![Cell::EMPTY; CELLS],
            section: 0,
        }
    }
}

impl LightCache {
    /// Starts on section `s`: fills the 20³ from `neighborhood`.
    pub fn start(&mut self, neighborhood: &Neighborhood, s: usize) {
        self.section = s;
        self.fill(neighborhood);
    }

    fn fill(&mut self, neighborhood: &Neighborhood) {
        let mut block_lv = [0u8; CELLS];
        let mut sky_lv = [0u8; CELLS];
        scatter_light(neighborhood, self.section, false, &mut block_lv);
        scatter_light(neighborhood, self.section, true, &mut sky_lv);

        let s = self.section;
        if let Some((flat_s, flat)) = neighborhood.decoded_center()
            && flat_s == s
        {
            for y in 0..16i32 {
                for z in 0..16i32 {
                    let row = Section::index(0, y as usize, z as usize);
                    for x in 0..16i32 {
                        self.write(x, y, z, flat[row + x as usize], &block_lv, &sky_lv);
                    }
                }
            }
        } else if let Some(id) = neighborhood.chunks[4]
            .and_then(|chunk| chunk.sections.get(s))
            .and_then(|section| section.uniform())
        {
            for y in 0..16i32 {
                for z in 0..16i32 {
                    for x in 0..16i32 {
                        self.write(x, y, z, id, &block_lv, &sky_lv);
                    }
                }
            }
        } else {
            for y in 0..16i32 {
                for z in 0..16i32 {
                    for x in 0..16i32 {
                        let state = neighborhood.block(s, x, y, z).unwrap_or(0);
                        self.write(x, y, z, state, &block_lv, &sky_lv);
                    }
                }
            }
        }

        for y in -SHELL..16 + SHELL {
            for z in -SHELL..16 + SHELL {
                for x in -SHELL..16 + SHELL {
                    if ((x | y | z) as u32) < 16 {
                        continue;
                    }
                    let state = neighborhood.block(s, x, y, z).unwrap_or(0);
                    self.write(x, y, z, state, &block_lv, &sky_lv);
                }
            }
        }
    }

    #[inline]
    fn write(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        state: u16,
        block_lv: &[u8; CELLS],
        sky_lv: &[u8; CELLS],
    ) {
        let i = cell_index(x, y, z);
        let flags = ao_flags::of(state);
        let block = block_lv[i];
        let sky = sky_lv[i];
        let light = if flags.emissive_rendering() {
            FULL_BRIGHT
        } else {
            pack(u32::from(block.max(flags.emission())), u32::from(sky))
        };
        self.cells[i] = Cell {
            light,
            flags,
            block,
            sky,
        };
    }

    /// The cell at section-relative `(x, y, z)`, each `-2..=17`.
    fn at(&self, [x, y, z]: [i32; 3]) -> Cell {
        self.cells[cell_index(x, y, z)]
    }

    /// The flags of the block at section-relative `pos`.
    pub fn flags(&self, pos: [i32; 3]) -> AoFlags {
        self.at(pos).flags
    }
}

/// Unpacks each 16³ light slab that overlaps the cache into `dest`.
fn scatter_light(neighborhood: &Neighborhood, s: usize, sky: bool, dest: &mut [u8; CELLS]) {
    let mut unpacked = [0u8; BLOCKS_PER_SECTION];
    for cz in -1..=1i32 {
        for cx in -1..=1i32 {
            let Some(chunk) = neighborhood.chunks[((cx + 1) + (cz + 1) * 3) as usize] else {
                continue;
            };
            let layers = if sky {
                chunk.light.sky.as_slice()
            } else {
                chunk.light.block.as_slice()
            };
            let has_skylight = chunk.light.has_skylight();
            for ds in -1..=1i32 {
                unpack_slot(layers, s as i32 + 1 + ds, sky, has_skylight, &mut unpacked);
                scatter_slab(dest, &unpacked, cx, cz, ds);
            }
        }
    }
}

fn unpack_slot(
    layers: &[LightSection],
    slot: i32,
    sky: bool,
    has_skylight: bool,
    out: &mut [u8; BLOCKS_PER_SECTION],
) {
    match usize::try_from(slot).ok().and_then(|i| layers.get(i)) {
        Some(LightSection::Data(data)) => nibble_decode::unpack(data, out),
        Some(LightSection::Empty) if sky && has_skylight && slot as usize + 1 == layers.len() => {
            out.fill(15);
        }
        None if sky && has_skylight && slot >= layers.len() as i32 => out.fill(15),
        _ => out.fill(0),
    }
}

/// The overlap of one 16³ slab (`(cx, ds, cz)` in chunks) with the 20³ cache.
fn scatter_slab(
    dest: &mut [u8; CELLS],
    unpacked: &[u8; BLOCKS_PER_SECTION],
    cx: i32,
    cz: i32,
    ds: i32,
) {
    let y_base = ds * 16;
    let x_base = cx * 16;
    let z_base = cz * 16;
    let y_lo = y_base.max(-SHELL);
    let y_hi = (y_base + 16).min(16 + SHELL);
    let x_lo = x_base.max(-SHELL);
    let x_hi = (x_base + 16).min(16 + SHELL);
    let z_lo = z_base.max(-SHELL);
    let z_hi = (z_base + 16).min(16 + SHELL);
    if y_lo >= y_hi || x_lo >= x_hi || z_lo >= z_hi {
        return;
    }
    let width = (x_hi - x_lo) as usize;
    let lx0 = (x_lo - x_base) as usize;
    for y in y_lo..y_hi {
        let ly = (y - y_base) as usize;
        for z in z_lo..z_hi {
            let lz = (z - z_base) as usize;
            let src = Section::index(lx0, ly, lz);
            let dst = cell_index(x_lo, y, z);
            dest[dst..dst + width].copy_from_slice(&unpacked[src..src + width]);
        }
    }
}

/// `AdjacencyInfo.corners`: the four sides a face's corners lie towards.
fn adjacent(direction: Direction) -> [Direction; 4] {
    use Direction::*;
    match direction {
        Down => [West, East, North, South],
        Up => [East, West, North, South],
        North => [Up, Down, East, West],
        South => [West, East, Down, Up],
        West => [Up, Down, North, South],
        East => [Down, Up, North, South],
    }
}

fn add(a: [i32; 3], d: Direction) -> [i32; 3] {
    let [dx, dy, dz] = d.step();
    [a[0] + dx, a[1] + dy, a[2] + dz]
}

/// Smooth coordinates as a corner: quarter levels, AO in sixtieths that
fn corner(smooth: u32, ao: f32, emission: u8) -> CornerLight {
    let (mut block, mut sky) = (smooth & 0xFF, smooth >> 16 & 0xFF);
    if emission != 0 {
        let emission = u32::from(emission);
        block = (block >> 4).max(emission) << 4;
        sky = (sky >> 4).max(emission) << 4;
    }
    let quarter = |v: u32| ((v + 2) / 4).min(60) as u8;
    let gray = (ao * 255.0).floor() as u32;
    let ao = (gray * 60).div_ceil(255).min(60) as u8;
    CornerLight {
        block: quarter(block),
        sky: quarter(sky),
        ao,
    }
}

/// A face's four corners, smooth lit: `prepareQuadAmbientOcclusion` for
pub fn smooth_corners(
    cache: &LightCache,
    shape: &AoShape,
    pos: [i32; 3],
    center: AoFlags,
    emission: u8,
) -> [CornerLight; 4] {
    let groups = shape.groups;
    let direction = shape.direction;
    let cubic = shape.cubic(center.collision_full());
    let base = if cubic { add(pos, direction) } else { pos };
    let [c0, c1, c2, c3] = adjacent(direction);
    let cell = |p: [i32; 3]| cache.at(p);

    let n = [c0, c1, c2, c3].map(|c| cell(add(base, c)));
    let permeable =
        [c0, c1, c2, c3].map(|c| cell(add(add(base, c), direction)).flags.light_permeable());
    let (light, shade) = (n.map(|c| c.light), n.map(Cell::shade));
    let diagonal = |a: usize, ca: Direction, b: usize, cb: Direction| {
        if !permeable[a] && !permeable[b] {
            (shade[0], light[0])
        } else {
            let c = cell(add(add(base, ca), cb));
            (c.shade(), c.light)
        }
    };
    let (shade02, light02) = diagonal(2, c0, 0, c2);
    let (shade03, light03) = diagonal(3, c0, 0, c3);
    let (shade12, light12) = diagonal(2, c1, 1, c2);
    let (shade13, light13) = diagonal(3, c1, 1, c3);

    let mut light_center = cell(pos).light;
    let next = cell(add(pos, direction));
    if cubic || !next.flags.solid_render() {
        light_center = next.light;
    }
    let shade_center = cell(base).shade();

    let ao = [
        (shade[3] + shade[0] + shade03 + shade_center) * 0.25,
        (shade[2] + shade[0] + shade02 + shade_center) * 0.25,
        (shade[2] + shade[1] + shade12 + shade_center) * 0.25,
        (shade[3] + shade[1] + shade13 + shade_center) * 0.25,
    ];
    let blended = [
        smooth_blend(light[3], light[0], light03, light_center),
        smooth_blend(light[2], light[0], light02, light_center),
        smooth_blend(light[2], light[1], light12, light_center),
        smooth_blend(light[3], light[1], light13, light_center),
    ];
    std::array::from_fn(|i| {
        if shape.partial {
            let w = shape.weights[i];
            let ao = (ao[0] * w[0] + ao[1] * w[1] + ao[2] * w[2] + ao[3] * w[3]).clamp(0.0, 1.0);
            corner(smooth_weighted_blend(blended, w), ao, emission)
        } else {
            let k = usize::from(groups[i]);
            corner(blended[k], ao[k], emission)
        }
    })
}

/// A face's corners, flat lit (`prepareQuadFlat`): one light for all four.
pub fn flat_corners(
    cache: &LightCache,
    light_pos: [i32; 3],
    center: AoFlags,
    emission: u8,
) -> [CornerLight; 4] {
    let cell = cache.at(light_pos);
    let light = if center.emissive_rendering() {
        FULL_BRIGHT
    } else {
        pack(
            u32::from(cell.block.max(center.emission())),
            u32::from(cell.sky),
        )
    };
    let smooth = (block_of(light) << 4) | (sky_of(light) << 4) << 16;
    [corner(smooth, 1.0, emission); 4]
}

/// What the old lazy fill stored for one cell, for the tests.
#[cfg(test)]
fn compute_cell(neighborhood: &Neighborhood, s: usize, [x, y, z]: [i32; 3]) -> (u32, AoFlags) {
    let state = neighborhood.block(s, x, y, z).unwrap_or(0);
    let flags = ao_flags::of(state);
    let light = if flags.emissive_rendering() {
        FULL_BRIGHT
    } else {
        let (block, sky) = neighborhood.light(s, x, y, z);
        pack(u32::from(block.max(flags.emission())), u32::from(sky))
    };
    (light, flags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::chunk::{Chunk, ChunkLight};
    use crate::world::section::Section;

    #[test]
    fn blending_follows_vanilla() {
        let lit = pack(0, 15);
        assert_eq!(smooth_blend(lit, lit, lit, lit), 240 << 16);
        assert_eq!(smooth_blend(0, lit, lit, lit), 240 << 16);
        let dim = pack(0, 2);
        assert_eq!(smooth_blend(0, 0, 0, dim) >> 16, 8);
        let torch = pack(14, 0);
        let out = smooth_blend(torch, lit, lit, lit);
        assert_eq!(out & 0xFF, 14 * 16 / 4);
        assert_eq!(out >> 16, 240);
    }

    #[test]
    fn weighted_blend_wraps_negative_like_java() {
        assert_eq!(
            smooth_weighted_blend([16, 0, 0, 0], [-1.0, 0.0, 0.0, 0.0]),
            240
        );
        assert_eq!(
            smooth_weighted_blend([16 << 16, 0, 0, 0], [-1.0, 0.0, 0.0, 0.0]),
            240 << 16
        );
        assert_eq!(
            smooth_weighted_blend([240, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
            240
        );
    }

    #[test]
    fn corners_store_vanilla_gray() {
        for (ao, gray) in [
            (1.0f32, 255u32),
            (0.8, 204),
            (0.6, 153),
            (0.4, 102),
            (0.2, 51),
        ] {
            let c = corner(0, ao, 0);
            assert_eq!(u32::from(c.ao) * 255 / 60, gray, "ao {ao}");
        }
        let c = corner((7 * 16) | ((12 * 16) << 16), 1.0, 0);
        assert_eq!((c.block, c.sky), (28, 48));
        let c = corner(3 * 16, 1.0, 15);
        assert_eq!((c.block, c.sky), (60, 60));
    }

    #[test]
    fn a_filled_cache_matches_the_old_cell_by_cell_lookups() {
        let mut sections: Vec<Section> = (0..3).map(|_| Section::empty()).collect();
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if (x + y + z) % 5 == 0 {
                        sections[1].set(Section::index(x, y, z), 1);
                    }
                }
            }
        }
        sections[0].set(Section::index(14, 15, 3), 1);
        sections[2].set(Section::index(1, 0, 8), 1);
        let mut light = ChunkLight::new(crate::world::dimension::Height {
            min_y: crate::world::dimension::Height::OVERWORLD.min_y,
            height: sections.len() as i32 * 16,
        });
        for y in -80i32..-80 + 3 * 16 + 32 {
            for z in 0..16i32 {
                for x in 0..16i32 {
                    let block = ((x + y.rem_euclid(16) + z) % 16) as u8;
                    let sky = 15 - block;
                    light.set(x, y, z, block, sky);
                }
            }
        }
        let center = Chunk::new(0, 0, sections).with_light(light);
        let mut west_sections: Vec<Section> = (0..3).map(|_| Section::empty()).collect();
        west_sections[1].set(Section::index(15, 8, 8), 1);
        let west = Chunk::new(-1, 0, west_sections);
        let mut chunks = [None; 9];
        chunks[4] = Some(&center);
        chunks[3] = Some(&west);
        let mut flat = Box::new([0u16; BLOCKS_PER_SECTION]);
        center.sections[1].copy_to(&mut flat);
        let neighborhood = Neighborhood::with_chunks(chunks, Some((1, &*flat)));

        let mut cache = LightCache::default();
        cache.start(&neighborhood, 1);
        for y in -SHELL..16 + SHELL {
            for z in -SHELL..16 + SHELL {
                for x in -SHELL..16 + SHELL {
                    let (light, flags) = compute_cell(&neighborhood, 1, [x, y, z]);
                    let cell = cache.at([x, y, z]);
                    assert_eq!(cell.light, light, "light at {x},{y},{z}");
                    assert_eq!(cell.flags, flags, "flags at {x},{y},{z}");
                }
            }
        }
    }

    #[test]
    fn empty_sky_padding_fills_the_cache_like_vanilla() {
        let chunk = Chunk::new(0, 0, vec![Section::empty()]);
        let neighborhood = Neighborhood::alone(&chunk);
        let mut cache = LightCache::default();
        cache.start(&neighborhood, 0);
        let cell = cache.at([8, 16, 8]);
        assert_eq!(cell.sky, 15);
        assert_eq!(sky_of(cell.light), 15);
        let below = cache.at([8, 0, 8]);
        assert_eq!(below.sky, 0);
    }
}
