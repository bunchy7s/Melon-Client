//! The world's 2D layers: HUD, chat, the open overlay, F3, and the item

use super::camera::CameraMode;
use super::world_resources::WorldResources;
use super::{DebugFrame, InWorld};
use crate::gui::hud::item_slot::ItemPainter;
use crate::gui::overlay::{OverlayDraw, OverlayKind, OverlayTextures};
use crate::i18n::Lang;
use crate::renderer::Renderer;
use crate::renderer::quad::DrawList;
use crate::settings::options::Config;
use crate::world::player::{self};
use std::time::Instant;

impl InWorld {
    /// The HUD, the chat and the action bar, and the overlay over them.
    pub fn draw_ui(
        &mut self,
        draw_list: &mut DrawList,
        assets: &WorldResources,
        painter: ItemPainter,
        lang: &Lang,
        config: &Config,
        size: (f32, f32),
    ) {
        self.effects_frame.draw(draw_list, &assets.effects, size);
        let tag_background = config.chat.world_background_opacity(0.25);
        self.draw_text_displays(draw_list, painter, lang, size, tag_background);
        self.draw_name_tags(draw_list, painter, lang, size, tag_background);
        let panel = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.effects_panel())
            .filter(|panel| panel.fits(size.0));
        if panel.is_none() {
            crate::gui::hud::effects::draw_hud(
                draw_list,
                &assets.hud.effects,
                &self.status_effects,
                size.0,
            );
        }
        self.hud.draw(
            draw_list,
            &assets.hud,
            painter,
            &self.inventory,
            &self.status,
            crate::gui::hud::AttackCooldown {
                strength: self.hands.attack_strength(self.inventory.selected_stack()),
                indicator: config.attack_indicator,
                main_arm: config.main_hand,
                target_ready: self.attack_target_ready(),
            },
            self.camera_mode == CameraMode::FirstPerson && !self.debug.visible(),
            size,
        );
        if self.game_mode.current.is_spectator() {
            self.spectator_menu.draw(
                draw_list,
                &assets.spectator,
                assets.hud.hotbar_sprites(),
                &|id| assets.players.for_profile(id).texture,
                painter,
                lang,
                size,
            );
        }
        self.draw_sidebar(draw_list, painter, lang, size);
        let partial = self.tick_time / player::TICK;
        let (font, pages, white) = (painter.font, painter.font_textures, painter.white);
        self.hud
            .draw_overlay_message(draw_list, painter, size, partial);
        self.hud
            .bosses
            .draw(draw_list, &assets.hud.boss, painter, size, self.hud.ticks());
        self.hud.draw_title(draw_list, painter, size, partial);
        let chat_open = self.overlay_kind() == Some(OverlayKind::Chat);
        self.chat
            .draw(draw_list, font, pages, white, lang, size.1, chat_open);
        self.draw_tab_list(draw_list, assets, painter, lang, size.0);
        if let Some(overlay) = &mut self.overlay {
            overlay.draw(&mut OverlayDraw {
                draw_list,
                textures: OverlayTextures {
                    inventory: &assets.inventory,
                    creative: &assets.creative,
                    switcher: &assets.switcher,
                    container: &assets.container,
                },
                painter,
                inventory: &self.inventory,
                container: self.open_container.as_ref(),
                chat: &self.chat,
                chat_settings: &config.chat,
                lang,
                size,
                recipes: &self.recipes,
                recipe_sets: &self.recipe_sets,
                ghost: self.ghost_recipe.as_ref(),
                experience_level: self.status.experience_level,
                infinite_materials: self.game_mode.current.has_infinite_materials(),
                entity_preview: self.preview.current,
            });
        }
        let effects_tooltip = panel.and_then(|panel| {
            crate::gui::hud::effects::draw_inventory(
                draw_list,
                &assets.hud.effects,
                painter,
                lang,
                &self.status_effects,
                &panel,
                size.0,
            )
            .map(|lines| (lines, panel.mouse))
        });
        if let Some((lines, mouse)) = effects_tooltip {
            painter.tooltip_lines(draw_list, &lines, mouse, size);
        }
    }

    /// The stacks drawn in the player's hands, each with whether it is in
    pub fn held_stacks(
        &self,
    ) -> Vec<(
        &pumpkin_data::item_stack::ItemStack,
        crate::world::item_model::HeldUse,
    )> {
        use crate::world::player::hands::Hand;
        let mut stacks = Vec::with_capacity(4);
        for hand in Hand::ALL {
            let using = self.hands.using_in(hand).is_some();
            let held = self.hand_stack(hand);
            stacks.push((held, self.held_use(hand, held, using)));
            let shown = self.hands.shown(hand);
            stacks.push((shown, self.held_use(hand, shown, using)));
        }
        stacks
    }

    /// Hands the stacks drawn this frame to `visit`, for baking their
    pub fn visit_shown_stacks(
        &self,
        visit: &mut dyn FnMut(&pumpkin_data::item_stack::ItemStack) -> bool,
    ) {
        let inventory = &self.inventory;
        let mut go = |stack: &pumpkin_data::item_stack::ItemStack| stack.is_empty() || visit(stack);
        let fixed = (0..9)
            .map(|i| inventory.hotbar(i))
            .chain([&inventory.slots()[crate::world::inventory::OFFHAND_SLOT]])
            .all(&mut go);
        if fixed && let Some(menu) = &self.open_container {
            let more = menu
                .slots
                .iter()
                .all(|stack| stack.is_empty() || visit(stack));
            if !more {
                return;
            }
        }
        if fixed && let Some(overlay) = &self.overlay {
            overlay.visit_stacks(inventory, visit);
        }
    }

    /// The F3 overlay, when shown. Its lines are only gathered then.
    pub fn draw_debug(
        &self,
        draw_list: &mut DrawList,
        gui: &crate::gui::assets::GuiAssets,
        renderer: &Renderer,
        frame: DebugFrame,
        width: f32,
    ) {
        if !self.debug.visible() {
            return;
        }
        let context = crate::gui::hud::debug::DebugContext {
            fps: self.fps.fps,
            framerate_limit: frame.framerate_limit,
            vsync: frame.vsync,
            present_mode: renderer.present_mode_name(),
            position: self.player.pos(),
            yaw_degrees: self.player.yaw(),
            pitch_degrees: self.player.pitch(),
            framebuffer: renderer.framebuffer_size(),
            device: renderer.device_info(),
            dimension: &self.dimension,
            render: frame.render,
            mesh: self.level.mesh_stats(),
            mesh_memory: renderer.mesh_memory(),
            profile: self.profiler.lines(),
            gpu_profile: renderer.gpu_profile(),
            subsystems: self.subsystems.lines().to_vec(),
            day: self.clocks.environment(0.0).day,
            day_time: self.clocks.environment(0.0).day_time,
            extras: self.debug_extras(frame.clouds, frame.biome_blend),
        };
        self.debug.draw(
            draw_list,
            &gui.font,
            &gui.font_textures,
            gui.white,
            &context,
            width,
        );
    }
}

/// `Minecraft.getFps()`: frames completed in the last whole second, not an
pub(super) struct FpsCounter {
    pub(super) frames: u32,
    pub(super) last_second: Instant,
    pub(super) fps: u32,
}

impl FpsCounter {
    pub(super) fn new() -> Self {
        Self {
            frames: 0,
            last_second: Instant::now(),
            fps: 0,
        }
    }

    pub(super) fn tick(&mut self, now: Instant) {
        self.frames += 1;
        if now.duration_since(self.last_second).as_secs_f32() >= 1.0 {
            self.fps = self.frames;
            self.frames = 0;
            self.last_second = now;
        }
    }
}
