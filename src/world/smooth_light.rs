//! Vanilla's smooth lighting and ambient occlusion for block faces

use crate::renderer::CornerLight;
use crate::world::block::ao_flags::AoFlags;
use crate::world::block::direction::Direction;
use crate::world::block_models::ao_shape::AoShape;
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

/// `LightCoordsUtil.smoothWeightedBlend` of four smooth coordinates.
fn smooth_weighted_blend(coords: [u32; 4], weights: [f32; 4]) -> u32 {
    let mut sky = 0.0f32;
    let mut block = 0.0f32;
    for (c, w) in coords.into_iter().zip(weights) {
        sky += (c >> 16 & 0xFF) as f32 * w;
        block += (c & 0xFF) as f32 * w;
    }
    (block as u32 & 0xFF) | (sky as u32 & 0xFF) << 16
}

/// One cell of the cache: its light coordinates as vanilla's
#[derive(Clone, Copy)]
struct Cell {
    light: u32,
    flags: AoFlags,
}

impl Cell {
    /// `getShadeBrightness`.
    fn shade(self) -> f32 {
        if self.flags.shade_dark() { 0.2 } else { 1.0 }
    }
}

/// How far around the section faces look: a face whose light comes from
const SHELL: i32 = 2;
/// Side of the cached cube: a section plus the shell on every side.
const SIDE: usize = 16 + 2 * SHELL as usize;

/// TODO(perf): smooth light is ~45% of meshing (`mesh_chunk` 1.19 ms vs
/// 0.66 ms flat, `perf_world` 2026-11-15). Every cell is filled lazily behind
/// an `Option` (12 bytes; [`Self::start`] clears 20³ of them, ~94 KiB, every
/// section), each fill one `Neighborhood::block` and one nibble lookup.
/// Plan: fill the 20³ light and flag cells densely once a section, the
/// inside from the decoded section and the light layers unpacked with the
/// byte kernels (`math::bytes`), the shell from the neighbours; then
/// `smooth_corners`' four-corner averages fit one 128-bit register, add
/// and multiply only, so still bit for bit vanilla's.
/// The light and flags of a section's blocks and their neighbours, filled
pub struct LightCache {
    cells: Vec<Option<Cell>>,
    flags: &'static [AoFlags],
    section: usize,
}

impl Default for LightCache {
    fn default() -> Self {
        Self {
            cells: vec![None; SIDE * SIDE * SIDE],
            flags: crate::world::block::ao_flags::table(),
            section: 0,
        }
    }
}

impl LightCache {
    /// Starts on section `s`: forgets the last one's cells.
    pub fn start(&mut self, s: usize) {
        self.cells.fill(None);
        self.section = s;
    }

    /// The cell at section-relative `(x, y, z)`, each `-2..=17`.
    fn at(&mut self, neighborhood: &Neighborhood, [x, y, z]: [i32; 3]) -> Cell {
        let [ix, iy, iz] = [x, y, z].map(|v| {
            debug_assert!((-SHELL..16 + SHELL).contains(&v), "{v} outside the cache");
            (v + SHELL) as usize
        });
        let i = (iy * SIDE + iz) * SIDE + ix;
        if let Some(cell) = self.cells[i] {
            return cell;
        }
        let s = self.section;
        let state = neighborhood.block(s, x, y, z).unwrap_or(0);
        let flags = self
            .flags
            .get(usize::from(state))
            .copied()
            .unwrap_or_else(|| crate::world::block::ao_flags::of(state));
        let light = if flags.emissive_rendering() {
            FULL_BRIGHT
        } else {
            let (block, sky) = neighborhood.light(s, x, y, z);
            pack(u32::from(block.max(flags.emission())), u32::from(sky))
        };
        let cell = Cell { light, flags };
        self.cells[i] = Some(cell);
        cell
    }

    /// The flags of the block at section-relative `pos`.
    pub fn flags(&mut self, neighborhood: &Neighborhood, pos: [i32; 3]) -> AoFlags {
        self.at(neighborhood, pos).flags
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
    cache: &mut LightCache,
    neighborhood: &Neighborhood,
    shape: &AoShape,
    pos: [i32; 3],
    center: AoFlags,
    emission: u8,
) -> Option<[CornerLight; 4]> {
    let groups = shape.groups?;
    let direction = shape.direction;
    let cubic = shape.cubic(center.collision_full());
    let base = if cubic { add(pos, direction) } else { pos };
    let [c0, c1, c2, c3] = adjacent(direction);
    let mut cell = |p: [i32; 3]| cache.at(neighborhood, p);

    let n = [c0, c1, c2, c3].map(|c| cell(add(base, c)));
    let permeable =
        [c0, c1, c2, c3].map(|c| cell(add(add(base, c), direction)).flags.light_permeable());
    let (light, shade) = (n.map(|c| c.light), n.map(Cell::shade));
    let mut diagonal = |a: usize, ca: Direction, b: usize, cb: Direction| {
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
    Some(std::array::from_fn(|i| {
        if shape.partial {
            let w = shape.weights[i];
            let ao = (ao[0] * w[0] + ao[1] * w[1] + ao[2] * w[2] + ao[3] * w[3]).clamp(0.0, 1.0);
            corner(smooth_weighted_blend(blended, w), ao, emission)
        } else {
            let k = usize::from(groups[i]);
            corner(blended[k], ao[k], emission)
        }
    }))
}

/// A face's corners, flat lit (`prepareQuadFlat`): one light for all four.
pub fn flat_corners(
    cache: &mut LightCache,
    neighborhood: &Neighborhood,
    light_pos: [i32; 3],
    center: AoFlags,
    emission: u8,
) -> [CornerLight; 4] {
    let light = if center.emissive_rendering() {
        FULL_BRIGHT
    } else {
        let (block, sky) =
            neighborhood.light(cache.section, light_pos[0], light_pos[1], light_pos[2]);
        pack(u32::from(block.max(center.emission())), u32::from(sky))
    };
    let smooth = (block_of(light) << 4) | (sky_of(light) << 4) << 16;
    [corner(smooth, 1.0, emission); 4]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
