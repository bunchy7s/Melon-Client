//! What vanilla's smooth lighting reads off a quad's geometry

use glam::Vec3;

use crate::world::block::direction::Direction;
use crate::world::geometry::closest_direction;

/// The facts above for one quad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AoShape {
    /// `BakedQuad.direction`: the axis its normal points most along
    pub direction: Direction,
    /// `facePartial`: it doesn't cover its side of the block.
    pub partial: bool,
    /// Flat along its own axis (min == max), the first half of `faceCubic`.
    pub flat: bool,
    /// On the block's side along its axis: the rest of `faceCubic`, unless
    pub boundary: bool,
    /// Per vertex, the corner group (vanilla's `vert0..vert3`) it takes;
    pub groups: Option<[u8; 4]>,
    /// Per vertex, its four group weights for partial quads
    pub weights: [[f32; 4]; 4],
}

const D: usize = 0;
const U: usize = 1;
const N: usize = 2;
const S: usize = 3;
const W: usize = 4;
const E: usize = 5;
const FD: usize = 6;
const FU: usize = 7;
const FN: usize = 8;
const FS: usize = 9;
const FW: usize = 10;
const FE: usize = 11;

/// `AdjacencyInfo.vert0Weights..vert3Weights` by direction: four pairs
const WEIGHTS: [[[usize; 8]; 4]; 6] = [
    [
        [FW, S, FW, FS, W, FS, W, S],
        [FW, N, FW, FN, W, FN, W, N],
        [FE, N, FE, FN, E, FN, E, N],
        [FE, S, FE, FS, E, FS, E, S],
    ],
    [
        [E, S, E, FS, FE, FS, FE, S],
        [E, N, E, FN, FE, FN, FE, N],
        [W, N, W, FN, FW, FN, FW, N],
        [W, S, W, FS, FW, FS, FW, S],
    ],
    [
        [U, FW, U, W, FU, W, FU, FW],
        [U, FE, U, E, FU, E, FU, FE],
        [D, FE, D, E, FD, E, FD, FE],
        [D, FW, D, W, FD, W, FD, FW],
    ],
    [
        [U, FW, FU, FW, FU, W, U, W],
        [D, FW, FD, FW, FD, W, D, W],
        [D, FE, FD, FE, FD, E, D, E],
        [U, FE, FU, FE, FU, E, U, E],
    ],
    [
        [U, S, U, FS, FU, FS, FU, S],
        [U, N, U, FN, FU, FN, FU, N],
        [D, N, D, FN, FD, FN, FD, N],
        [D, S, D, FS, FD, FS, FD, S],
    ],
    [
        [FD, S, FD, FS, D, FS, D, S],
        [FD, N, FD, FN, D, FN, D, N],
        [FU, N, FU, FN, U, FN, U, N],
        [FU, S, FU, FS, U, FS, U, S],
    ],
];

/// `AmbientVertexRemap`: the `FaceInfo` slot each group `vert0..vert3`
const REMAP: [[usize; 4]; 6] = [
    [0, 1, 2, 3],
    [2, 3, 0, 1],
    [3, 0, 1, 2],
    [0, 1, 2, 3],
    [3, 0, 1, 2],
    [1, 2, 3, 0],
];

/// `FaceInfo`: each slot's corner as max (true) or min per axis x, y, z.
const FACE_INFO: [[[bool; 3]; 4]; 6] = [
    [
        [false, false, true],
        [false, false, false],
        [true, false, false],
        [true, false, true],
    ],
    [
        [false, true, false],
        [false, true, true],
        [true, true, true],
        [true, true, false],
    ],
    [
        [true, true, false],
        [true, false, false],
        [false, false, false],
        [false, true, false],
    ],
    [
        [false, true, true],
        [false, false, true],
        [true, false, true],
        [true, true, true],
    ],
    [
        [false, true, false],
        [false, false, false],
        [false, false, true],
        [false, true, true],
    ],
    [
        [true, true, true],
        [true, false, true],
        [true, false, false],
        [true, true, false],
    ],
];

impl AoShape {
    /// The shape of a quad with block-space `corners` (`0..1`, before any
    pub fn of(corners: [Vec3; 4]) -> Self {
        let normal = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        let direction = closest_direction(normal).unwrap_or(Direction::Up);
        let min = corners.iter().fold(Vec3::splat(32.0), |m, &c| m.min(c));
        let max = corners.iter().fold(Vec3::splat(-32.0), |m, &c| m.max(c));
        let size = [
            min.y,
            max.y,
            min.z,
            max.z,
            min.x,
            max.x,
            1.0 - min.y,
            1.0 - max.y,
            1.0 - min.z,
            1.0 - max.z,
            1.0 - min.x,
            1.0 - max.x,
        ];
        let (lo, hi) = (1.0e-4, 0.9999);
        let axis = direction.axis();
        let plane = match axis {
            0 => [1, 2],
            1 => [0, 2],
            _ => [0, 1],
        };
        let partial = plane.iter().any(|&a| min[a] >= lo || max[a] <= hi);
        let flat = min[axis] == max[axis];
        let boundary = if direction.is_positive() {
            max[axis] > hi
        } else {
            min[axis] < lo
        };

        let d = direction.index();
        let table = &WEIGHTS[d];
        let group_weights: [[f32; 4]; 4] = std::array::from_fn(|k| {
            std::array::from_fn(|j| size[table[k][2 * j]] * size[table[k][2 * j + 1]])
        });
        let mid = (min + max) * 0.5;
        let mut groups = [0u8; 4];
        let mut seen = 0u8;
        let mut matched = max[plane[0]] > min[plane[0]] && max[plane[1]] > min[plane[1]];
        for (i, corner) in corners.iter().enumerate() {
            let side = [corner.x > mid.x, corner.y > mid.y, corner.z > mid.z];
            let slot = FACE_INFO[d]
                .iter()
                .position(|info| plane.iter().all(|&a| info[a] == side[a]));
            let Some(slot) = slot else {
                matched = false;
                break;
            };
            let group = REMAP[d].iter().position(|&s| s == slot).unwrap_or(0);
            seen |= 1 << group;
            groups[i] = group as u8;
        }
        matched &= seen == 0b1111;
        let weights = std::array::from_fn(|i| group_weights[usize::from(groups[i])]);
        Self {
            direction,
            partial,
            flat,
            boundary,
            groups: matched.then_some(groups),
            weights,
        }
    }

    /// `faceCubic`: the light and shade come from the block in front.
    pub fn cubic(&self, collision_full: bool) -> bool {
        self.flat && (self.boundary || collision_full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn a_full_top_face_maps_its_corners_like_vanilla() {
        let shape = AoShape::of([v(0., 1., 0.), v(0., 1., 1.), v(1., 1., 1.), v(1., 1., 0.)]);
        assert!(!shape.partial && shape.flat && shape.boundary);
        assert!(shape.cubic(false));
        assert_eq!(shape.groups, Some([2, 3, 0, 1]));
    }

    #[test]
    fn vertex_order_doesnt_matter() {
        let shape = AoShape::of([v(1., 1., 1.), v(1., 1., 0.), v(0., 1., 0.), v(0., 1., 1.)]);
        assert_eq!(shape.groups, Some([0, 1, 2, 3]));
    }

    #[test]
    fn a_slab_top_is_whole_but_not_on_the_side() {
        let shape = AoShape::of([
            v(0., 0.5, 0.),
            v(0., 0.5, 1.),
            v(1., 0.5, 1.),
            v(1., 0.5, 0.),
        ]);
        assert!(!shape.partial && shape.flat && !shape.boundary);
        assert!(!shape.cubic(false) && shape.cubic(true));
    }

    #[test]
    fn a_small_face_is_partial_and_weighs_its_corners() {
        let (a, b) = (7.0 / 16.0, 9.0 / 16.0);
        let shape = AoShape::of([
            v(a, 0.625, a),
            v(a, 0.625, b),
            v(b, 0.625, b),
            v(b, 0.625, a),
        ]);
        assert!(shape.partial);
        for weights in shape.weights {
            assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn rotated_quads_find_their_nearest_axis() {
        let shape = AoShape::of([v(0., 0., 0.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 0.)]);
        assert_eq!(shape.direction, Direction::South);
    }
}
