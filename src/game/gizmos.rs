//! Debug lines in the world (vanilla's `Gizmos`): entity hitboxes

use glam::{DVec3, IVec3, Mat4};

use crate::renderer::{EntityDraw, MeshId, RenderLayer, Renderer, WorldVertex};
use crate::world::physics::Aabb;

/// `GizmoStyle.stroke`'s width, in pixels on a 1920 wide window.
pub const DEFAULT_WIDTH: f32 = 2.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: DVec3,
    pub b: DVec3,
    /// `ARGB.colorFromFloat`: red, green, blue, alpha.
    pub color: [f32; 4],
    /// Pixels wide on a 1920 wide window.
    pub width: f32,
}

/// The lines of one frame.
#[derive(Default)]
pub struct Gizmos {
    lines: Vec<Line>,
}

impl Gizmos {
    pub fn line(&mut self, a: DVec3, b: DVec3, color: [f32; 4], width: f32) {
        self.lines.push(Line { a, b, color, width });
    }

    /// `Gizmos.cuboid`: the twelve edges.
    pub fn cuboid(&mut self, bounds: &Aabb, color: [f32; 4], width: f32) {
        let (lo, hi) = (bounds.min, bounds.max);
        let corner = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        for (y, z) in [(false, false), (true, false), (false, true), (true, true)] {
            self.line(corner(false, y, z), corner(true, y, z), color, width);
        }
        for (x, z) in [(false, false), (true, false), (false, true), (true, true)] {
            self.line(corner(x, false, z), corner(x, true, z), color, width);
        }
        for (x, y) in [(false, false), (true, false), (false, true), (true, true)] {
            self.line(corner(x, y, false), corner(x, y, true), color, width);
        }
    }

    /// `Gizmos.point`: a small cross.
    pub fn point(&mut self, at: DVec3, color: [f32; 4], size: f32) {
        let arm = 0.02 * f64::from(size);
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            self.line(at - axis * arm, at + axis * arm, color, size);
        }
    }

    /// `Gizmos.arrow`: a line and two barbs back from its tip.
    pub fn arrow(&mut self, from: DVec3, to: DVec3, color: [f32; 4], width: f32) {
        self.line(from, to, color, width);
        let along = to - from;
        let length = along.length();
        if length < 1.0e-6 {
            return;
        }
        let dir = along / length;
        let across = dir
            .cross(if dir.y.abs() < 0.9 {
                DVec3::Y
            } else {
                DVec3::X
            })
            .normalize();
        let up = dir.cross(across);
        let head = 0.1_f64.min(length / 3.0);
        for barb in [across, -across, up, -up] {
            self.line(to, to - dir * head + barb * head * 0.5, color, width);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

/// The mesh of the last frame's lines.
#[derive(Default)]
pub struct GizmoMesh {
    mesh: Option<MeshId>,
    built: Vec<WorldVertex>,
    origin: IVec3,
}

impl GizmoMesh {
    /// `gizmos` seen from `camera`, into `out`. `per_pixel` is how many
    pub fn draw(
        &mut self,
        renderer: &mut Renderer,
        gizmos: &Gizmos,
        camera: DVec3,
        per_pixel: f32,
        scale: f32,
        out: &mut Vec<EntityDraw>,
    ) {
        if gizmos.is_empty() {
            self.release(renderer);
            return;
        }
        let origin = camera.floor().as_ivec3();
        let vertices = ribbons(&gizmos.lines, origin, camera, per_pixel, scale);
        if vertices.is_empty() {
            self.release(renderer);
            return;
        }
        if self.mesh.is_none() || self.origin != origin || self.built != vertices {
            self.release(renderer);
            match renderer.upload_mesh(&vertices) {
                Ok(mesh) => {
                    self.mesh = Some(mesh);
                    self.built = vertices;
                    self.origin = origin;
                }
                Err(e) => eprintln!("Gizmo-Mesh fehlgeschlagen: {e:#}"),
            }
        }
        if let Some(mesh) = self.mesh {
            out.push(EntityDraw {
                mesh,
                atlas: renderer.white_texture(),
                model: Mat4::IDENTITY,
                origin,
                layer: RenderLayer::Shadows,
                blend: crate::renderer::Blending::Opaque,
                light: None,
                overlay: 0.0,
                glint: crate::renderer::Glint::None,
            });
        }
    }

    pub fn release(&mut self, renderer: &mut Renderer) {
        if let Some(mesh) = self.mesh.take() {
            renderer.free_mesh(mesh);
        }
        self.built.clear();
    }
}

/// One quad per line, facing `camera`, relative to `origin`.
fn ribbons(
    lines: &[Line],
    origin: IVec3,
    camera: DVec3,
    per_pixel: f32,
    scale: f32,
) -> Vec<WorldVertex> {
    let origin = origin.as_dvec3();
    let mut out = Vec::with_capacity(lines.len() * 4);
    for line in lines {
        let along = line.b - line.a;
        let length = along.length();
        if length < 1.0e-9 {
            continue;
        }
        let dir = along / length;
        let to_camera = camera - (line.a + line.b) / 2.0;
        let distance = to_camera.length();
        let across = dir.cross(to_camera);
        if across.length_squared() < 1.0e-18 {
            continue;
        }
        let across = across.normalize();
        let half = f64::from(line.width * scale.max(1.0) * per_pixel) * distance / 2.0;
        let (a, b) = (line.a - dir * half - origin, line.b + dir * half - origin);
        let mut quad = [
            a - across * half,
            b - across * half,
            b + across * half,
            a + across * half,
        ];
        let normal = (quad[1] - quad[0]).cross(quad[3] - quad[0]);
        if normal.dot(to_camera) < 0.0 {
            quad.reverse();
        }
        out.extend(
            quad.iter()
                .map(|p| WorldVertex::rgba(p.as_vec3().to_array(), [0.0, 0.0], line.color)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cuboid_is_twelve_ribbons() {
        let mut gizmos = Gizmos::default();
        gizmos.cuboid(
            &Aabb {
                min: DVec3::ZERO,
                max: DVec3::ONE,
            },
            [1.0; 4],
            DEFAULT_WIDTH,
        );
        assert_eq!(gizmos.len(), 12);
        let vertices = ribbons(
            &gizmos.lines,
            IVec3::ZERO,
            DVec3::new(3.0, 4.0, 5.0),
            0.001,
            1.0,
        );
        assert_eq!(vertices.len(), 48);
    }

    #[test]
    fn a_ribbon_is_as_wide_on_screen_wherever_it_is() {
        let width_at = |distance: f64| {
            let line = Line {
                a: DVec3::new(0.0, 0.0, -distance),
                b: DVec3::new(0.0, 1.0, -distance),
                color: [1.0; 4],
                width: 2.0,
            };
            let v = ribbons(&[line], IVec3::ZERO, DVec3::ZERO, 0.002, 1.0);
            (v[0].pos[0] - v[1].pos[0])
                .abs()
                .max((v[0].pos[0] - v[3].pos[0]).abs())
        };
        for distance in [2.0_f64, 20.0] {
            let expected = 2.0 * 0.002 * (distance * distance + 0.25).sqrt();
            let width = f64::from(width_at(distance));
            assert!((width - expected).abs() < 1.0e-4, "{width} {expected}");
        }
    }
}
