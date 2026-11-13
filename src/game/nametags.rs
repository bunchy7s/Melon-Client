//! The name tags over entities (`NameTagFeatureRenderer`): a player's

use glam::{DVec3, Mat4, Vec3, Vec4Swizzles};

use super::InWorld;
use super::camera::{Camera, CameraMode};
use crate::component::Component;
use crate::gui::hud::item_slot::ItemPainter;
use crate::gui::text::{self, StyledText};
use crate::i18n::Lang;
use crate::renderer::quad::{DrawList, argb};
use crate::world::entity::kind::EntityKind;
use crate::world::entity::tracker::ClientEntity;
use crate::world::pick;
use crate::world::scoreboard::Visibility;

/// `EntityRenderer.submitNameTag`'s `scale(0.025)`: blocks a text pixel.
const PIXEL: f32 = 0.025;
/// `EntityRenderDispatcher.distanceToSqr < 64 * 64`.
const RANGE: f32 = 64.0;
/// `Attributes.BELOW_NAME_DISTANCE` default.
const BELOW_NAME_RANGE: f32 = 10.0;
/// `submitNameDisplay`: 9 * 1.15 after the score, in tag pixels.
const SCORE_GAP: f32 = 9.0 * 1.15;
/// `Options.getBackgroundColor(0.25F)`.
const BACKGROUND: u32 = 0x4000_0000;
/// The see-through pass: white at `0x20` alpha.
const BEHIND_ALPHA: f32 = 32.0 / 255.0;

/// The camera of the frame, enough to project a world point to the screen.
#[derive(Clone, Copy, Debug)]
pub struct NameView {
    position: Vec3,
    /// `bobView * view`: the world as the camera sees it.
    view: Mat4,
    tan_half_fov: f32,
    aspect: f32,
}

impl NameView {
    pub fn of(camera: &Camera, aspect: f32) -> Self {
        Self {
            position: camera.position,
            view: camera.bob * camera.view,
            tan_half_fov: (camera.fov.to_radians() / 2.0).tan(),
            aspect,
        }
    }

    /// Where `point` falls on a GUI screen of `size`, and how far in front
    pub fn project(&self, point: Vec3, size: (f32, f32)) -> Option<([f32; 2], f32)> {
        let v = (self.view * (point - self.position).extend(1.0)).xyz();
        let depth = -v.z;
        if depth < crate::renderer::WORLD_Z_NEAR {
            return None;
        }
        let x = v.x / (depth * self.tan_half_fov * self.aspect);
        let y = v.y / (depth * self.tan_half_fov);
        if x.abs() > 1.5 || y.abs() > 1.5 {
            return None;
        }
        Some(([size.0 / 2.0 * (1.0 + x), size.1 / 2.0 * (1.0 - y)], depth))
    }

    /// GUI pixels one pixel of tag text covers at `depth`.
    pub fn text_scale(&self, depth: f32, size: (f32, f32)) -> f32 {
        PIXEL * size.1 / 2.0 / (self.tan_half_fov * depth)
    }
}

impl InWorld {
    /// The shown custom names of the entities in range, as they hang above
    pub(super) fn draw_name_tags(
        &self,
        draw_list: &mut DrawList,
        painter: ItemPainter,
        lang: &Lang,
        size: (f32, f32),
    ) {
        let Some(view) = self.name_view else {
            return;
        };
        let partial = self.tick_time / crate::world::player::TICK;
        for entity in self.entities.iter() {
            let name = self.name_tag_component(entity);
            let holder = self.score_holder(entity);
            let feet = entity.pos(partial);
            let height = f64::from(entity.size().1);
            let anchor = (feet + DVec3::new(0.0, height + 0.5, 0.0)).as_vec3();
            let dist_sq = anchor.distance_squared(view.position);
            if dist_sq >= RANGE * RANGE {
                continue;
            }
            let score = if dist_sq < BELOW_NAME_RANGE * BELOW_NAME_RANGE {
                holder.and_then(|holder| self.scoreboard.below_name_display(holder))
            } else {
                None
            };
            if name.is_none() && score.is_none() {
                continue;
            }
            let Some((at, depth)) = view.project(anchor, size) else {
                continue;
            };
            let discrete = entity.appearance.crouching();
            let occluded = pick::clip_blocks_for_nametag(
                &self.level,
                view.position.as_dvec3(),
                anchor.as_dvec3(),
            )
            .is_some();
            if discrete && occluded {
                continue;
            }
            let scale = view.text_scale(depth, size);
            if let Some(score) = score {
                let styled = StyledText::from_component(&score, lang);
                if !styled.is_empty() {
                    self.draw_tag(
                        draw_list,
                        painter,
                        &styled,
                        [at[0], at[1] + SCORE_GAP * scale],
                        scale,
                        false,
                    );
                }
            }
            if let Some(name) = name {
                let styled = StyledText::from_component(&name, lang);
                if styled.is_empty() {
                    continue;
                }
                self.draw_tag(draw_list, painter, &styled, at, scale, false);
            }
        }
    }

    /// The text displays in range (`TextDisplayRenderer`): lines split at
    pub(super) fn draw_text_displays(
        &self,
        draw_list: &mut DrawList,
        painter: ItemPainter,
        lang: &Lang,
        size: (f32, f32),
    ) {
        use crate::world::entity::display::{
            FLAG_ALIGN_LEFT, FLAG_ALIGN_RIGHT, FLAG_DEFAULT_BACKGROUND, FLAG_SEE_THROUGH,
            FLAG_SHADOW,
        };
        let Some(view) = self.name_view else {
            return;
        };
        let partial = self.tick_time / crate::world::player::TICK;
        for entity in self.entities.iter() {
            if entity.kind != EntityKind::TextDisplay {
                continue;
            }
            let Some(state) = entity.display.as_deref() else {
                continue;
            };
            if state.flags & FLAG_SEE_THROUGH == 0 {
                continue;
            }
            let tick = entity.age_in_ticks(0.0) as i32;
            let transform = state.transform(tick, partial);
            let forward = -Vec3::new(view.view.x_axis.z, view.view.y_axis.z, view.view.z_axis.z);
            let facing = crate::world::entity::display::orientation(
                state.billboard,
                (entity.yaw_degrees(partial), entity.pitch_degrees(partial)),
                crate::world::entity::display::camera_angles(forward),
            );
            let anchor = entity.pos(partial).as_vec3() + facing * transform.translation;
            if anchor.distance_squared(view.position) >= (state.view_range * 64.0).powi(2) {
                continue;
            }
            let Some((at, depth)) = view.project(anchor, size) else {
                continue;
            };
            let full = StyledText::from_component(&state.text, lang);
            if full.is_empty() {
                continue;
            }
            let lines = text::wrap(painter.font, &full, state.line_width.max(1) as f32);
            let widths: Vec<f32> = lines.iter().map(|l| text::width(painter.font, l)).collect();
            let box_w = widths.iter().copied().fold(0.0_f32, f32::max);
            let box_h = lines.len() as f32 * 10.0 - 1.0;
            let up = Vec3::new(view.view.x_axis.y, view.view.y_axis.y, view.view.z_axis.y);
            let middle = anchor + up * (box_h * 0.5 * 0.025 * transform.scale.y.abs().max(1e-3));
            let hidden = pick::clip_blocks_for_nametag(
                &self.level,
                view.position.as_dvec3(),
                middle.as_dvec3(),
            )
            .is_some();
            if crate::probe::enabled() {
                crate::probe::note("text_display", format!("#{}", entity.id), || {
                    format!(
                        "pos={:?} billboard={} translation={:?} scale={:?} left={:?} right={:?} anchor={anchor:?} hidden={hidden}",
                        entity.pos(partial),
                        state.billboard,
                        transform.translation,
                        transform.scale,
                        transform.left,
                        transform.right,
                    )
                });
            }
            let see_through = state.flags & FLAG_SEE_THROUGH != 0;
            if hidden && !see_through {
                continue;
            }
            let opacity = f32::from(state.opacity as u8) / 255.0;
            let alpha = if hidden { BEHIND_ALPHA } else { opacity };
            let scale =
                view.text_scale(depth, size) * transform.scale.y.abs().max(1e-3) / PIXEL * PIXEL;
            let background = if state.flags & FLAG_DEFAULT_BACKGROUND != 0 {
                BACKGROUND
            } else {
                state.background as u32
            };
            if background >> 24 != 0 {
                draw_list.push_fill(
                    painter.white,
                    at[0] + (1.0 - box_w / 2.0 - 1.0) * scale,
                    at[1] - (box_h + 1.0) * scale,
                    box_w * scale,
                    box_h * scale,
                    argb(background),
                );
            }
            let start = draw_list.vertices_len();
            let mut paint = text::TextPaint {
                font: painter.font,
                pages: painter.font_textures,
                white: painter.white,
                color: [1.0; 3],
                alpha,
                shadow: state.flags & FLAG_SHADOW != 0,
                obfuscation_seed: 0,
            };
            paint.obfuscation_seed = entity.id as u64;
            for (i, (line, width)) in lines.iter().zip(&widths).enumerate() {
                let x = if state.flags & FLAG_ALIGN_LEFT != 0 {
                    0.0
                } else if state.flags & FLAG_ALIGN_RIGHT != 0 {
                    box_w - width
                } else {
                    box_w / 2.0 - width / 2.0
                };
                text::draw(
                    draw_list,
                    &paint,
                    line,
                    1.0 - box_w / 2.0 + x,
                    -box_h + 10.0 * i as f32,
                );
            }
            draw_list.scale_since(start, scale);
            draw_list.offset_since(start, at);
        }
    }

    /// `EntityRenderer.getNameTag` / `LivingEntityRenderer.shouldShowName`.
    fn name_tag_component(&self, entity: &ClientEntity) -> Option<Component> {
        if entity.kind == EntityKind::Other {
            return None;
        }
        if entity.kind == EntityKind::Player {
            return self.player_name_tag(entity);
        }
        if !entity.appearance.name_visible {
            return None;
        }
        entity.custom_name.as_deref().cloned()
    }

    /// `ScoreHolder.getScoreboardName`: a player's profile name.
    fn score_holder<'a>(&'a self, entity: &'a ClientEntity) -> Option<&'a str> {
        if entity.kind != EntityKind::Player {
            return None;
        }
        self.players.get(&entity.uuid).map(|p| p.name.as_str())
    }

    /// `Player.shouldShowName` is always true; the tag is the team-coloured
    fn player_name_tag(&self, entity: &ClientEntity) -> Option<Component> {
        if entity.uuid == self.uuid && self.camera_mode == CameraMode::FirstPerson {
            return None;
        }
        let player = self.players.get(&entity.uuid)?;
        if player.game_mode.is_spectator() {
            return None;
        }
        if !self.team_shows_name_tag(&player.name, entity.appearance.invisible()) {
            return None;
        }
        Some(
            self.scoreboard
                .format_name(&player.name, Component::text(player.name.clone())),
        )
    }

    /// `LivingEntityRenderer.shouldShowName`'s team branch.
    fn team_shows_name_tag(&self, holder: &str, invisible: bool) -> bool {
        let own = self
            .players
            .get(&self.uuid)
            .and_then(|p| self.scoreboard.team_of(&p.name));
        let theirs = self.scoreboard.team_of(holder);
        let visible = !invisible
            || (own.is_some_and(|(_, team)| team.look.sees_friendly_invisibles())
                && own.is_some_and(|(name, _)| theirs.is_some_and(|(theirs, _)| theirs == name)));
        let Some((_, team)) = theirs else {
            return visible;
        };
        let same = matches!((own, theirs), (Some((a, _)), Some((b, _))) if a == b);
        match team.look.name_tags {
            Visibility::Always => visible,
            Visibility::Never => false,
            Visibility::HideForOtherTeams => {
                if own.is_none() {
                    visible
                } else {
                    same && (team.look.sees_friendly_invisibles() || visible)
                }
            }
            Visibility::HideForOwnTeam => {
                if own.is_none() {
                    visible
                } else {
                    !same && visible
                }
            }
        }
    }

    fn draw_tag(
        &self,
        draw_list: &mut DrawList,
        painter: ItemPainter,
        styled: &StyledText,
        at: [f32; 2],
        scale: f32,
        hidden: bool,
    ) {
        let width = text::width(painter.font, styled);
        let (left, top) = (-width / 2.0 - 1.0, -1.0);
        let (box_w, box_h) = (width + 2.0, 10.0);
        draw_list.push_fill(
            painter.white,
            at[0] + left * scale,
            at[1] + top * scale - box_h * scale,
            box_w * scale,
            box_h * scale,
            argb(BACKGROUND),
        );
        let start = draw_list.vertices_len();
        let paint = text::TextPaint {
            font: painter.font,
            pages: painter.font_textures,
            white: painter.white,
            color: [1.0; 3],
            alpha: if hidden { BEHIND_ALPHA } else { 1.0 },
            shadow: false,
            obfuscation_seed: 0,
        };
        text::draw(draw_list, &paint, styled, -width / 2.0, -9.0);
        draw_list.scale_since(start, scale);
        draw_list.offset_since(start, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> NameView {
        NameView {
            position: Vec3::ZERO,
            view: Mat4::IDENTITY,
            tan_half_fov: (35.0_f32).to_radians().tan(),
            aspect: 16.0 / 9.0,
        }
    }

    #[test]
    fn a_point_ahead_is_in_the_middle_and_one_behind_is_not_drawn() {
        let size = (320.0, 180.0);
        let (at, depth) = view().project(Vec3::new(0.0, 0.0, -5.0), size).unwrap();
        assert!((at[0] - 160.0).abs() < 1e-3 && (at[1] - 90.0).abs() < 1e-3);
        assert!((depth - 5.0).abs() < 1e-6);
        assert!(view().project(Vec3::new(0.0, 0.0, 5.0), size).is_none());
        let (above, _) = view().project(Vec3::new(0.0, 1.0, -5.0), size).unwrap();
        assert!(above[1] < 90.0);
    }

    #[test]
    fn the_text_shrinks_with_distance_like_the_world_quad() {
        let size = (320.0, 180.0);
        let near = view().text_scale(2.0, size);
        let far = view().text_scale(4.0, size);
        assert!((near / far - 2.0).abs() < 1e-5);
    }
}
