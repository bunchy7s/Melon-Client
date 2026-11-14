//! Debug lines in the world (vanilla's `Gizmos`): entity hitboxes

use glam::{DVec3, IVec3, Mat4, Vec3};

use crate::gui::text::font::Font;
use crate::renderer::quad::{DrawList, Vertex};
use crate::renderer::{
    Blending, EntityDraw, MeshId, RenderLayer, Renderer, TextureId, WorldVertex,
};
use crate::world::block::direction::Direction;
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

/// A filled quad (`GizmoStyle.fill`), its corners in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fill {
    pub corners: [DVec3; 4],
    pub color: [f32; 4],
}

/// `TextGizmo`: text turned to the camera, centred on `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldText {
    pub at: DVec3,
    pub text: String,
    pub color: [f32; 4],
    /// `TextGizmo.Style.scale`: sixteenths of a block per text pixel.
    pub scale: f32,
}

/// `TextGizmo.Style.DEFAULT_SCALE`.
pub const TEXT_SCALE: f32 = 0.32;

/// The lines, faces and text of one frame.
#[derive(Default)]
pub struct Gizmos {
    lines: Vec<Line>,
    fills: Vec<Fill>,
    texts: Vec<WorldText>,
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

    /// `Gizmos.rect` with a fill: the face of the box `min`..`max` toward
    /// `face`.
    pub fn face(&mut self, min: DVec3, max: DVec3, face: Direction, color: [f32; 4]) {
        let (lo, hi) = (min, max);
        let corners = match face {
            Direction::Down => [
                DVec3::new(lo.x, lo.y, lo.z),
                DVec3::new(hi.x, lo.y, lo.z),
                DVec3::new(hi.x, lo.y, hi.z),
                DVec3::new(lo.x, lo.y, hi.z),
            ],
            Direction::Up => [
                DVec3::new(lo.x, hi.y, lo.z),
                DVec3::new(lo.x, hi.y, hi.z),
                DVec3::new(hi.x, hi.y, hi.z),
                DVec3::new(hi.x, hi.y, lo.z),
            ],
            Direction::North => [
                DVec3::new(lo.x, lo.y, lo.z),
                DVec3::new(lo.x, hi.y, lo.z),
                DVec3::new(hi.x, hi.y, lo.z),
                DVec3::new(hi.x, lo.y, lo.z),
            ],
            Direction::South => [
                DVec3::new(lo.x, lo.y, hi.z),
                DVec3::new(hi.x, lo.y, hi.z),
                DVec3::new(hi.x, hi.y, hi.z),
                DVec3::new(lo.x, hi.y, hi.z),
            ],
            Direction::West => [
                DVec3::new(lo.x, lo.y, lo.z),
                DVec3::new(lo.x, lo.y, hi.z),
                DVec3::new(lo.x, hi.y, hi.z),
                DVec3::new(lo.x, hi.y, lo.z),
            ],
            Direction::East => [
                DVec3::new(hi.x, lo.y, lo.z),
                DVec3::new(hi.x, hi.y, lo.z),
                DVec3::new(hi.x, hi.y, hi.z),
                DVec3::new(hi.x, lo.y, hi.z),
            ],
        };
        self.fills.push(Fill { corners, color });
    }

    /// `Gizmos.cuboid` with a fill: all six faces.
    pub fn filled_cuboid(&mut self, bounds: &Aabb, color: [f32; 4]) {
        for face in Direction::ALL {
            self.face(bounds.min, bounds.max, face, color);
        }
    }

    /// `Gizmos.billboardText`.
    pub fn text(&mut self, at: DVec3, text: impl Into<String>, color: [f32; 4], scale: f32) {
        self.texts.push(WorldText {
            at,
            text: text.into(),
            color,
            scale,
        });
    }

    pub fn extend_fills(&mut self, fills: &[Fill]) {
        self.fills.extend_from_slice(fills);
    }

    pub fn extend_lines(&mut self, lines: &[Line]) {
        self.lines.extend_from_slice(lines);
    }

    pub fn extend_texts(&mut self, texts: &[WorldText]) {
        self.texts.extend_from_slice(texts);
    }

    pub fn take_fills(&mut self) -> Vec<Fill> {
        std::mem::take(&mut self.fills)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

/// The meshes of the last frame's gizmos: the lines, the filled faces
/// (blended), and the text, one mesh per font page.
#[derive(Default)]
pub struct GizmoMesh {
    lines: Slot,
    fills: Slot,
    texts: Vec<(TextureId, Slot)>,
}

/// The font and the camera's axes, for [`Gizmos::text`].
#[derive(Clone, Copy)]
pub struct TextView<'a> {
    pub font: &'a Font,
    pub pages: &'a [TextureId],
    /// The camera's right and up, in the world.
    pub right: Vec3,
    pub up: Vec3,
}

/// One uploaded mesh, kept while its vertices stay the same.
#[derive(Default)]
struct Slot {
    mesh: Option<MeshId>,
    built: Vec<WorldVertex>,
    origin: IVec3,
}

impl Slot {
    fn update(&mut self, renderer: &mut Renderer, vertices: Vec<WorldVertex>, origin: IVec3) {
        if vertices.is_empty() {
            self.release(renderer);
            return;
        }
        if self.mesh.is_some() && self.origin == origin && self.built == vertices {
            return;
        }
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

    fn release(&mut self, renderer: &mut Renderer) {
        if let Some(mesh) = self.mesh.take() {
            renderer.free_mesh(mesh);
        }
        self.built.clear();
    }

    fn draw(&self, atlas: TextureId, layer: RenderLayer, blend: Blending) -> Option<EntityDraw> {
        Some(EntityDraw {
            mesh: self.mesh?,
            atlas,
            model: Mat4::IDENTITY,
            origin: self.origin,
            layer,
            blend,
            light: None,
            overlay: 0.0,
            glint: crate::renderer::Glint::None,
        })
    }
}

impl GizmoMesh {
    /// `gizmos` seen from `camera`, into `out`. `per_pixel` is how many
    /// blocks a pixel covers one block ahead; `scale` the window's width
    /// over 1920. Text needs `text`; without it, none is drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        renderer: &mut Renderer,
        gizmos: &Gizmos,
        camera: DVec3,
        per_pixel: f32,
        scale: f32,
        text: Option<TextView<'_>>,
        out: &mut Vec<EntityDraw>,
    ) {
        let origin = camera.floor().as_ivec3();
        let white = renderer.white_texture();
        let lines = ribbons(&gizmos.lines, origin, camera, per_pixel, scale);
        self.lines.update(renderer, lines, origin);
        let fills = fill_quads(&gizmos.fills, origin);
        self.fills.update(renderer, fills, origin);
        let mut pages: Vec<(TextureId, Vec<WorldVertex>)> = Vec::new();
        if let Some(view) = text {
            text_quads(&gizmos.texts, origin, view, &mut pages);
        }
        self.texts.retain_mut(|(texture, slot)| {
            if pages.iter().any(|(t, _)| t == texture) {
                true
            } else {
                slot.release(renderer);
                false
            }
        });
        for (texture, vertices) in pages {
            let at = match self.texts.iter().position(|(t, _)| *t == texture) {
                Some(at) => at,
                None => {
                    self.texts.push((texture, Slot::default()));
                    self.texts.len() - 1
                }
            };
            self.texts[at].1.update(renderer, vertices, origin);
        }
        out.extend(
            self.lines
                .draw(white, RenderLayer::Shadows, Blending::Opaque),
        );
        out.extend(
            self.fills
                .draw(white, RenderLayer::Shadows, Blending::Translucent),
        );
        for (texture, slot) in &self.texts {
            out.extend(slot.draw(*texture, RenderLayer::Entities, Blending::Text));
        }
    }

    pub fn release(&mut self, renderer: &mut Renderer) {
        self.lines.release(renderer);
        self.fills.release(renderer);
        for (_, mut slot) in self.texts.drain(..) {
            slot.release(renderer);
        }
    }
}

/// Both sides of each filled quad, relative to `origin`.
fn fill_quads(fills: &[Fill], origin: IVec3) -> Vec<WorldVertex> {
    let origin = origin.as_dvec3();
    let mut out = Vec::with_capacity(fills.len() * 8);
    for fill in fills {
        let q = fill
            .corners
            .map(|p| WorldVertex::rgba((p - origin).as_vec3().to_array(), [0.0, 0.0], fill.color));
        out.extend_from_slice(&q);
        out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
    }
    out
}

/// `GizmoFeatureRenderer.buildTexts`: each text laid out by the font, at
/// `scale / 16` blocks a pixel, turned to the camera; one vertex list per
/// font page.
fn text_quads(
    texts: &[WorldText],
    origin: IVec3,
    view: TextView<'_>,
    pages: &mut Vec<(TextureId, Vec<WorldVertex>)>,
) {
    let origin = origin.as_dvec3();
    let mut list = DrawList::new();
    for text in texts {
        list.clear();
        let width = view.font.text_width(&text.text);
        view.font.push_text_flat(
            &mut list,
            view.pages,
            &text.text,
            -width / 2.0,
            0.0,
            text.color,
        );
        let pixel = text.scale / 16.0;
        let at = (text.at - origin).as_vec3();
        let place = |v: &Vertex| {
            let p = at + view.right * (v.pos[0] * pixel) - view.up * (v.pos[1] * pixel);
            WorldVertex::rgba(p.to_array(), v.uv, v.color)
        };
        for call in &list.calls {
            let from = (call.first / 6 * 4) as usize;
            let quads = &list.vertices[from..from + (call.count / 6 * 4) as usize];
            let at = match pages.iter().position(|(t, _)| *t == call.texture) {
                Some(at) => at,
                None => {
                    pages.push((call.texture, Vec::new()));
                    pages.len() - 1
                }
            };
            for quad in quads.as_chunks::<4>().0 {
                let q = [
                    place(&quad[0]),
                    place(&quad[1]),
                    place(&quad[2]),
                    place(&quad[3]),
                ];
                pages[at].1.extend_from_slice(&q);
                pages[at].1.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
            }
        }
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
