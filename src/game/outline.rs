//! The outline of the block looked at (`LevelRenderer.renderHitOutline`):

use glam::{DVec3, IVec3, Mat4};

use crate::renderer::{EntityDraw, MeshId, RenderLayer, Renderer, WorldVertex};
use crate::world::level::Level;
use crate::world::physics::Aabb;
use crate::world::pick::{self, BlockHit};

/// `ARGB.black(102)`: the colour's alpha.
const ALPHA: f32 = 102.0 / 255.0;
/// A distance counts in tenths of a block.
const DISTANCE_STEP: f64 = 10.0;
/// How far from an edge its corners are looked at, to tell a real edge
const PROBE: f64 = 1.0e-4;

/// What the mesh was built for.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    pos: [i32; 3],
    state: u16,
    /// The bar width in tenths of a millimetre.
    width: i32,
}

#[derive(Default)]
pub struct BlockOutline {
    mesh: Option<(MeshId, Key)>,
}

impl BlockOutline {
    /// The outline of `hit`, if any, into `out`. `camera` is where the
    pub fn draw(
        &mut self,
        renderer: &mut Renderer,
        level: &Level,
        hit: Option<BlockHit>,
        camera: DVec3,
        line_width: f32,
        out: &mut Vec<EntityDraw>,
    ) {
        let hit = hit.filter(|hit| {
            let [x, y, z] = hit.pos;
            level.block_state(x, y, z).is_some_and(|state| state != 0)
        });
        let Some(hit) = hit else {
            self.release(renderer);
            return;
        };
        let [x, y, z] = hit.pos;
        let state = level.block_state(x, y, z).unwrap_or(0);
        let centre = DVec3::new(f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5);
        let distance = (centre - camera).length();
        let distance = (distance * DISTANCE_STEP).round() / DISTANCE_STEP;
        let width = distance as f32 * line_width;
        let key = Key {
            pos: hit.pos,
            state,
            width: (width * 10_000.0).round() as i32,
        };
        if self.mesh.is_none_or(|(_, built)| built != key) {
            self.release(renderer);
            let boxes: Vec<Aabb> = pick::outline_shapes(level, (x, y, z)).collect();
            let origin = IVec3::from_array(hit.pos);
            let vertices = bars(&edges(&boxes), origin, width / 2.0);
            if vertices.is_empty() {
                return;
            }
            match renderer.upload_mesh(&vertices) {
                Ok(mesh) => self.mesh = Some((mesh, key)),
                Err(e) => eprintln!("Umriss-Mesh fehlgeschlagen: {e:#}"),
            }
        }
        if let Some((mesh, _)) = self.mesh {
            out.push(EntityDraw {
                mesh,
                atlas: renderer.white_texture(),
                model: Mat4::IDENTITY,
                origin: IVec3::from_array(hit.pos),
                layer: RenderLayer::Shadows,
                blend: crate::renderer::Blending::Opaque,
                light: None,
                overlay: 0.0,
                glint: crate::renderer::Glint::None,
            });
        }
    }

    fn release(&mut self, renderer: &mut Renderer) {
        if let Some((mesh, _)) = self.mesh.take() {
            renderer.free_mesh(mesh);
        }
    }

    pub fn clear(&mut self, renderer: &mut Renderer) {
        self.release(renderer);
    }
}

/// An edge of a shape: its two ends.
type Edge = (DVec3, DVec3);

/// `VoxelShape.forAllEdges`: the edges of the union of `boxes`. A box's edge
fn edges(boxes: &[Aabb]) -> Vec<Edge> {
    let inside = |p: DVec3| {
        boxes
            .iter()
            .any(|b| (0..3).all(|i| p[i] >= b.min[i] - 1.0e-9 && p[i] <= b.max[i] + 1.0e-9))
    };
    let mut out: Vec<Edge> = Vec::new();
    for b in boxes {
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for &cu in &[b.min[u], b.max[u]] {
                for &cv in &[b.min[v], b.max[v]] {
                    let mut a = DVec3::ZERO;
                    a[axis] = b.min[axis];
                    a[u] = cu;
                    a[v] = cv;
                    let mut e = a;
                    e[axis] = b.max[axis];
                    let mid = (a + e) / 2.0;
                    let mut quarter = [[false; 2]; 2];
                    for (i, su) in [-1.0, 1.0].into_iter().enumerate() {
                        for (j, sv) in [-1.0, 1.0].into_iter().enumerate() {
                            let mut p = mid;
                            p[u] += su * PROBE;
                            p[v] += sv * PROBE;
                            quarter[i][j] = inside(p);
                        }
                    }
                    let count = quarter.iter().flatten().filter(|&&q| q).count();
                    let diagonal = count == 2 && quarter[0][0] == quarter[1][1];
                    if !(count == 1 || count == 3 || diagonal) {
                        continue;
                    }
                    let same = |p: Edge| {
                        (p.0.distance(a) < 1.0e-6 && p.1.distance(e) < 1.0e-6)
                            || (p.0.distance(e) < 1.0e-6 && p.1.distance(a) < 1.0e-6)
                    };
                    if !out.iter().any(|&p| same(p)) {
                        out.push((a, e));
                    }
                }
            }
        }
    }
    out
}

/// Every edge as a square bar of half width `half`, in quads relative to
fn bars(edges: &[Edge], origin: IVec3, half: f32) -> Vec<WorldVertex> {
    let origin = origin.as_dvec3();
    let half = f64::from(half);
    let mut out = Vec::with_capacity(edges.len() * 16);
    for &(a, e) in edges {
        let axis = (0..3).find(|&i| (e[i] - a[i]).abs() > 1.0e-9).unwrap_or(0);
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut a = a - origin;
        let mut e = e - origin;
        a[axis] -= half;
        e[axis] += half;
        for (du, dv) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let at = |end: DVec3, across: f64| {
                let mut p = end;
                if du != 0.0 {
                    p[u] += du * half;
                    p[v] += across * half;
                } else {
                    p[v] += dv * half;
                    p[u] += across * half;
                }
                p
            };
            let mut quad = [at(a, -1.0), at(e, -1.0), at(e, 1.0), at(a, 1.0)];
            let mut outward = DVec3::ZERO;
            outward[u] = du;
            outward[v] = dv;
            let normal = (quad[1] - quad[0]).cross(quad[3] - quad[0]);
            if normal.dot(outward) < 0.0 {
                quad.reverse();
            }
            out.extend(quad.iter().map(|p| {
                WorldVertex::rgba(p.as_vec3().to_array(), [0.0, 0.0], [0.0, 0.0, 0.0, ALPHA])
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn cube() -> Aabb {
        Aabb {
            min: DVec3::ZERO,
            max: DVec3::ONE,
        }
    }

    #[test]
    fn a_cube_has_twelve_edges() {
        assert_eq!(edges(&[cube()]).len(), 12);
    }

    #[test]
    fn two_boxes_forming_a_cube_leave_the_seam_out() {
        let lower = Aabb {
            min: DVec3::ZERO,
            max: DVec3::new(1.0, 0.5, 1.0),
        };
        let upper = Aabb {
            min: DVec3::new(0.0, 0.5, 0.0),
            max: DVec3::ONE,
        };
        let found = edges(&[lower, upper]);
        assert!(
            found.iter().all(|(a, b)| !(a.y == 0.5 && b.y == 0.5)),
            "{found:?}"
        );
    }

    #[test]
    fn a_stairs_step_keeps_its_inner_edge() {
        let lower = Aabb {
            min: DVec3::ZERO,
            max: DVec3::new(1.0, 0.5, 1.0),
        };
        let upper = Aabb {
            min: DVec3::new(0.0, 0.5, 0.5),
            max: DVec3::ONE,
        };
        let found = edges(&[lower, upper]);
        let has = |a: DVec3, b: DVec3| {
            found
                .iter()
                .any(|&(p, q)| (p == a && q == b) || (p == b && q == a))
        };
        assert!(has(DVec3::new(0.0, 0.5, 0.5), DVec3::new(1.0, 0.5, 0.5)));
        assert!(has(DVec3::new(0.0, 1.0, 0.5), DVec3::new(1.0, 1.0, 0.5)));
        assert!(!has(DVec3::new(0.0, 0.5, 1.0), DVec3::new(1.0, 0.5, 1.0)));
    }

    #[test]
    fn every_bar_side_faces_away_from_its_edge() {
        let found = edges(&[cube()]);
        let vertices = bars(&found, IVec3::ZERO, 0.01);
        assert_eq!(vertices.len(), 12 * 16);
        for bar in vertices.chunks(16) {
            let points: Vec<Vec3> = bar.iter().map(|v| Vec3::from_array(v.pos)).collect();
            let axis = points.iter().sum::<Vec3>() / 16.0;
            for quad in points.chunks(4) {
                let normal = (quad[1] - quad[0]).cross(quad[3] - quad[0]);
                let middle = (quad[0] + quad[2]) / 2.0;
                assert!(normal.dot(middle - axis) > 0.0, "{quad:?} faces inwards");
            }
        }
    }
}
