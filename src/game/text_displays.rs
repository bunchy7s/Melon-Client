//! Text displays (`DisplayRenderer.TextDisplayRenderer`) as meshes in the

use std::collections::HashMap;

use glam::{IVec3, Mat4, Vec3};

use super::InWorld;
use crate::gui::hud::item_slot::ItemPainter;
use crate::gui::text::{self, StyledText};
use crate::i18n::Lang;
use crate::math::ChunkHash;
use crate::renderer::quad::{DrawList, argb};
use crate::renderer::{EntityDraw, Glint, MeshId, RenderLayer, Renderer, TextureId, WorldVertex};
use crate::world::entity::display::{
    self, FLAG_ALIGN_LEFT, FLAG_ALIGN_RIGHT, FLAG_DEFAULT_BACKGROUND, FLAG_SEE_THROUGH, FLAG_SHADOW,
};
use crate::world::entity::kind::EntityKind;
use crate::world::lightmap::LightmapParams;

/// Blocks a pixel of text covers (`scale(-0.025)`).
const PIXEL: f32 = 0.025;
/// `getBackgroundOpacity(0.25)`.
const DEFAULT_BACKGROUND: u32 = 0x4000_0000;
/// Pixels each later layer (background, glyphs) lies in front of the last,
const LAYER: f32 = 0.01;

struct Kept {
    revision: u32,
    /// The interpolated opacity and background it was built with.
    style: (u8, u32),
    parts: Vec<(MeshId, TextureId)>,
    seen: bool,
}

#[derive(Default)]
pub struct TextDisplayMeshes {
    kept: HashMap<i32, Kept, ChunkHash>,
}

impl TextDisplayMeshes {
    fn free(renderer: &mut Renderer, kept: Kept) {
        for (mesh, _) in kept.parts {
            renderer.free_mesh(mesh);
        }
    }

    pub fn clear(&mut self, renderer: &mut Renderer) {
        for (_, kept) in self.kept.drain() {
            Self::free(renderer, kept);
        }
    }
}

/// The meshes of one text display, one per texture it draws from, in draw
fn build(
    state: &display::DisplayState,
    painter: ItemPainter,
    lang: &Lang,
    seed: u64,
    (opacity, background): (u8, u32),
) -> Vec<(TextureId, Vec<WorldVertex>)> {
    let full = StyledText::from_component(&state.text, lang);
    if full.is_empty() {
        return Vec::new();
    }
    let lines = text::wrap(painter.font, &full, state.line_width.max(1) as f32);
    let widths: Vec<f32> = lines.iter().map(|l| text::width(painter.font, l)).collect();
    let box_w = widths.iter().copied().fold(0.0_f32, f32::max);
    let box_h = lines.len() as f32 * 10.0 - 1.0;
    let mut list = DrawList::new();
    let background = if state.flags & FLAG_DEFAULT_BACKGROUND != 0 {
        DEFAULT_BACKGROUND
    } else {
        background
    };
    if background >> 24 != 0 {
        list.push_fill(
            painter.white,
            -box_w / 2.0,
            -box_h - 1.0,
            box_w,
            box_h,
            argb(background),
        );
    }
    let paint = text::TextPaint {
        font: painter.font,
        pages: painter.font_textures,
        white: painter.white,
        color: [1.0; 3],
        alpha: f32::from(opacity) / 255.0,
        shadow: state.flags & FLAG_SHADOW != 0,
        obfuscation_seed: seed,
    };
    for (i, (line, width)) in lines.iter().zip(&widths).enumerate() {
        let x = if state.flags & FLAG_ALIGN_LEFT != 0 {
            0.0
        } else if state.flags & FLAG_ALIGN_RIGHT != 0 {
            box_w - width
        } else {
            box_w / 2.0 - width / 2.0
        };
        text::draw(
            &mut list,
            &paint,
            line,
            1.0 - box_w / 2.0 + x,
            -box_h + 10.0 * i as f32,
        );
    }
    let mut parts: Vec<(TextureId, Vec<WorldVertex>)> = Vec::new();
    for (layer, call) in list.calls.iter().enumerate() {
        let z = LAYER * layer as f32;
        let from = (call.first / 6 * 4) as usize;
        let quads = &list.vertices[from..from + (call.count / 6 * 4) as usize];
        let mut vertices = Vec::with_capacity(quads.len() * 2);
        for quad in quads.chunks_exact(4) {
            let corner = |v: &crate::renderer::quad::Vertex| {
                WorldVertex::rgba([v.pos[0], v.pos[1], z], v.uv, v.color)
            };
            let q = [
                corner(&quad[0]),
                corner(&quad[1]),
                corner(&quad[2]),
                corner(&quad[3]),
            ];
            vertices.extend_from_slice(&q);
            vertices.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
        }
        parts.push((call.texture, vertices));
    }
    parts
}

impl InWorld {
    /// Builds again the meshes of the text displays whose text changed, and
    pub fn prepare_text_displays(
        &mut self,
        renderer: &mut Renderer,
        painter: ItemPainter,
        lang: &Lang,
    ) {
        let partial = self.tick_time / crate::world::player::TICK;
        for kept in self.text_displays.kept.values_mut() {
            kept.seen = false;
        }
        for entity in self.entities.iter() {
            if entity.kind != EntityKind::TextDisplay {
                continue;
            }
            let Some(state) = entity.display.as_deref() else {
                continue;
            };
            if state.flags & FLAG_SEE_THROUGH != 0 {
                continue;
            }
            let style = state.style(entity.age_in_ticks(0.0) as i32, partial);
            if let Some(kept) = self.text_displays.kept.get_mut(&entity.id)
                && kept.revision == state.revision
                && kept.style == style
            {
                kept.seen = true;
                continue;
            }
            if let Some(old) = self.text_displays.kept.remove(&entity.id) {
                TextDisplayMeshes::free(renderer, old);
            }
            let mut parts = Vec::new();
            let built = build(state, painter, lang, entity.id as u64, style);
            if crate::probe::enabled() {
                crate::probe::note("text_display", format!("#{} built", entity.id), || {
                    let styled = StyledText::from_component(&state.text, lang);
                    let fonts: Vec<&str> = styled
                        .spans
                        .iter()
                        .filter_map(|s| s.style.font.as_deref())
                        .collect();
                    format!(
                        "text={:?} fonts={fonts:?} width={} flags={:#x} opacity={} background={:#x} \
                         line_width={} view_range={} billboard={} parts={:?} pos={:?} vehicle={:?} raw={:?}",
                        styled.to_plain(),
                        text::width(painter.font, &styled),
                        state.flags,
                        style.0,
                        style.1,
                        state.line_width,
                        state.view_range,
                        state.billboard,
                        built.iter().map(|(_, v)| v.len()).collect::<Vec<_>>(),
                        entity.pos(partial),
                        entity.vehicle,
                        state.text,
                    )
                });
            }
            for (texture, vertices) in built {
                match renderer.upload_mesh(&vertices) {
                    Ok(mesh) => parts.push((mesh, texture)),
                    Err(e) => eprintln!("Text-Display-Mesh fehlgeschlagen: {e:#}"),
                }
            }
            self.text_displays.kept.insert(
                entity.id,
                Kept {
                    revision: state.revision,
                    style,
                    parts,
                    seen: true,
                },
            );
        }
        let gone: Vec<i32> = self
            .text_displays
            .kept
            .iter()
            .filter(|(_, k)| !k.seen)
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(kept) = self.text_displays.kept.remove(&id) {
                TextDisplayMeshes::free(renderer, kept);
            }
        }
    }

    /// The text displays in `visible`, placed as `submit_display` places
    pub(super) fn text_display_draws(
        &self,
        visible: &[i32],
        camera: Vec3,
        view: (f32, f32),
        partial: f32,
        params: &LightmapParams,
        out: &mut Vec<EntityDraw>,
    ) {
        if crate::probe::enabled() {
            for entity in self.entities.iter() {
                if entity.kind != EntityKind::TextDisplay {
                    continue;
                }
                let outcome = if entity.display.is_none() {
                    "no data"
                } else if !visible.contains(&entity.id) {
                    "culled"
                } else if !self.text_displays.kept.contains_key(&entity.id) {
                    "no mesh"
                } else {
                    "submitted"
                };
                crate::probe::note(
                    "text_display",
                    format!("#{} {outcome}", entity.id),
                    || {
                        let feet = entity.pos(partial).as_vec3();
                        let transform = entity
                            .display
                            .as_deref()
                            .map(|s| s.transform(entity.age_in_ticks(0.0) as i32, partial));
                        format!(
                            "pos={feet:?} dist={:.1} translation={:?} scale={:?}",
                            (feet - camera).length(),
                            transform.as_ref().map(|t| t.translation),
                            transform.as_ref().map(|t| t.scale),
                        )
                    },
                );
            }
        }
        for entity in visible.iter().filter_map(|&id| self.entities.get(id)) {
            if entity.kind != EntityKind::TextDisplay {
                continue;
            }
            let (Some(state), Some(kept)) = (
                entity.display.as_deref(),
                self.text_displays.kept.get(&entity.id),
            ) else {
                continue;
            };
            let feet = entity.pos(partial).as_vec3();
            if (feet - camera).length_squared() >= (state.view_range * 64.0).powi(2) {
                continue;
            }
            let origin: IVec3 = feet.floor().as_ivec3();
            let tick = entity.age_in_ticks(0.0) as i32;
            let facing = display::orientation(
                state.billboard,
                (entity.yaw_degrees(partial), entity.pitch_degrees(partial)),
                view,
            );
            let model = Mat4::from_translation(feet - origin.as_vec3())
                * Mat4::from_quat(facing)
                * state.transform(tick, partial).matrix()
                * Mat4::from_rotation_y(std::f32::consts::PI)
                * Mat4::from_scale(Vec3::splat(-PIXEL));
            let light = match state.light_override() {
                Some((block, sky)) => crate::world::lightmap::sample(block, sky, params),
                None => self.light_at_eye(feet + Vec3::Y * state.height * 0.5, 0, params),
            };
            for &(mesh, texture) in &kept.parts {
                out.push(EntityDraw {
                    mesh,
                    atlas: texture,
                    model,
                    origin,
                    layer: RenderLayer::Entities,
                    translucent: true,
                    light: Some(light),
                    overlay: 0.0,
                    glint: Glint::None,
                });
            }
        }
    }
}
