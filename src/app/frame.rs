//! One frame: the background join or a world that ended, then the menus

use std::time::{Duration, Instant};

use winit::event_loop::{ActiveEventLoop, ControlFlow};

use super::RunningState;
use crate::game::{DebugFrame, InWorld};
use crate::gui::hud::item_slot::ItemPainter;
use crate::gui::widgets::Ui;
use crate::renderer::{Blur, Frame};

/// `Options.menuBackgroundBlurriness`'s default: the blur radius, in pixels,
const MENU_BACKGROUND_BLURRINESS: f32 = 5.0;

impl RunningState {
    pub(super) fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        crate::probe::flush_if_due();
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame);
        self.last_frame = now;
        self.env.gui_assets.sync_font(&mut self.renderer);
        if let Some(resources) = &mut self.world_resources {
            crate::game::sync_head_objects(&self.env.gui_assets.font, &mut resources.players);
        }

        if let Some(action) = self.screens.top_mut().map(|s| s.poll()) {
            self.apply(action, event_loop);
        }
        if let Some(reason) = self.world.as_mut().and_then(InWorld::ended) {
            let reason = if reason == crate::network::play::TIMEOUT_REASON {
                self.env.lang.get("disconnect.timeout").to_owned()
            } else {
                reason
            };
            self.leave_world(Some(reason));
        }
        self.poll_server_packs();

        if self.world.is_some() {
            if self.draw_world(now, dt.as_secs_f32()) {
                self.finish_bench(event_loop);
                return;
            }
        } else {
            self.draw_menus(dt);
        }
        self.sync_cursor();
        self.sync_ime();
        self.sync_ime_area();
        match self.framerate_limit() {
            None => self.window.request_redraw(),
            Some(fps) => {
                let next = now + Duration::from_secs_f64(1.0 / f64::from(fps));
                self.next_frame = Some(next);
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }
        }
    }

    /// The top screen over the panorama, or over the gradient without one.
    fn draw_menus(&mut self, dt: Duration) {
        if let Some(background) = &mut self.background {
            background.tick(dt);
        }
        let scene = self.background.as_ref().map(|b| b.scene());
        if let Some(screen) = self.screens.top_mut() {
            screen.tick();
            let renderer = &mut self.renderer;
            screen.upload_images(&mut |image| renderer.load_texture(image).ok());
        }

        let scale = self.scale();
        let size = scale.virtual_size();
        self.draw_list.clear();
        if scene.is_none() {
            crate::gui::background::push_gradient(
                &mut self.draw_list,
                self.env.gui_assets.white,
                size.0,
                size.1,
            );
        }
        let mut blur = None;
        if let Some(screen) = self.screens.top() {
            let env = &self.env;
            blur = menu_blur(screen, &mut self.draw_list, scene.is_some());
            let mut ui = Ui::new(&mut self.draw_list, &env.gui_assets, &env.lang, size);
            screen.draw(&mut ui, &env.ctx(scale));
        }

        let frame = Frame {
            scene: scene.as_ref(),
            ui: &self.draw_list,
            ui_size: size,
            clear_color: crate::gui::background::CLEAR_COLOR,
            blur,
        };
        if let Err(e) = self.renderer.render(&frame) {
            eprintln!("Render-Fehler: {e:#}");
        }
    }

    /// The benchmark is done: its results, the world saved, the game ends.
    fn finish_bench(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(run) = self.bench.take() {
            run.finish();
        }
        if let Some(mut world) = self.world.take() {
            world.release(&mut self.renderer);
            drop(world);
        }
        event_loop.exit();
    }

    /// Draws a frame of the world. Whether the benchmark has finished.
    fn draw_world(&mut self, now: Instant, dt: f32) -> bool {
        let blocked = self.paused() || self.overlay_open();
        let framerate_limit = self.framerate_limit();
        let scale = self.scale();
        let size = scale.virtual_size();
        let env = &self.env;
        let received = match &mut self.world {
            Some(world) => {
                world.profiler.begin_at(now);
                world.receive(&mut self.renderer, &env.lang, &env.gui_assets.font)
            }
            None => return false,
        };
        self.after_receive(received);
        let env = &self.env;
        let (Some(world), Some(world_resources)) = (&mut self.world, &mut self.world_resources)
        else {
            return false;
        };
        world.profiler.mark("receive");
        let ticks = world.advance(dt, blocked, &env.world_context(size));
        let ears = world.listener();
        env.audio.set_listener(ears.position, ears.forward, ears.up);
        world.sample_subsystems(env.audio.stats());
        env.audio.set_subtitles(env.config.show_subtitles);
        for sound in world.take_sounds() {
            env.audio.play(&sound);
        }
        world.hear(env.audio.take_heard(), &env.lang);
        world_resources.animate(&mut self.renderer, ticks);
        if let Some(screen) = self.screens.top_mut() {
            screen.tick();
            let renderer = &mut self.renderer;
            screen.upload_images(&mut |image| renderer.load_texture(image).ok());
        }
        world.profiler.mark("tick");
        world.mesh(&mut self.renderer, world_resources);
        world.profiler.mark("mesh");
        world_resources.prepare_icons(&mut self.renderer, |visit| world.visit_shown_stacks(visit));
        for (stack, using) in world.held_stacks() {
            world_resources.prepare_held(&mut self.renderer, stack, using);
        }
        world.debug_view.gui_scale = scale.scale() as f32;
        world.prepare_entities(&mut self.renderer, world_resources);
        let own = world.profile_id();
        world_resources
            .players
            .request(own, world.profile_textures(own));
        world_resources.players.poll(&mut self.renderer);
        world.prepare_entity_preview(
            &mut self.renderer,
            world_resources,
            &env.config,
            scale.scale(),
        );
        world.profiler.mark("icons");
        let aspect = self.renderer.aspect_ratio();
        {
            let gui = &env.gui_assets;
            world.prepare_text_displays(
                &mut self.renderer,
                ItemPainter {
                    icons: &world_resources.icons,
                    font: &gui.font,
                    font_textures: &gui.font_textures,
                    white: gui.white,
                    tooltip: gui.tooltip,
                },
                &env.lang,
            );
        }
        let (scene, render) = world.scene(
            &mut self.renderer,
            world_resources,
            aspect,
            now,
            &env.config,
        );
        world.profiler.mark("scene");

        let gui = &env.gui_assets;
        let painter = ItemPainter {
            icons: &world_resources.icons,
            font: &gui.font,
            font_textures: &gui.font_textures,
            white: gui.white,
            tooltip: gui.tooltip,
        };
        let icon_path = if blocked {
            None
        } else {
            self.world_icon.due(world, now)
        };
        self.draw_list.clear();
        world.draw_ui(
            &mut self.draw_list,
            world_resources,
            painter,
            &env.lang,
            &env.config,
            size,
        );
        let mut blur = None;
        if let Some(screen) = self.screens.top() {
            blur = menu_blur(screen, &mut self.draw_list, true);
            let mut ui = Ui::new(&mut self.draw_list, gui, &env.lang, size);
            screen.draw(&mut ui, &env.ctx(scale));
        }
        let debug = DebugFrame {
            render,
            clouds: env.config.clouds,
            biome_blend: env.config.biome_blend,
            framerate_limit,
            vsync: env.config.vsync,
        };
        world.draw_debug(&mut self.draw_list, gui, &self.renderer, debug, size.0);
        world.profiler.mark("ui");
        if icon_path.is_some() {
            self.draw_list.clear();
            self.renderer.request_capture();
        }
        let probe_shot = if icon_path.is_none() {
            crate::probe::screenshot_due()
        } else {
            None
        };
        if probe_shot.is_some() {
            self.renderer.request_capture();
        }

        let frame = Frame {
            scene: Some(&scene),
            ui: &self.draw_list,
            ui_size: size,
            clear_color: world.sky_clear_color(),
            blur,
        };
        if let Err(e) = self.renderer.render(&frame) {
            eprintln!("Render-Fehler: {e:#}");
        }
        world.recycle(scene);
        if let Some(path) = probe_shot {
            if let Some(image) = self.renderer.take_capture()
                && let Err(e) = image.save(&path)
            {
                eprintln!("Tester: {} nicht geschrieben: {e}", path.display());
            }
        } else if let (Some(path), Some(image)) = (icon_path, self.renderer.take_capture()) {
            super::world_icon::save(&path, &image);
        }
        let waits = self.renderer.last_waits();
        world.profiler.mark_split(
            "render",
            &[
                ("gpu wait", waits.gpu),
                ("acquire wait", waits.acquire),
                ("present", waits.present),
            ],
        );
        world.profiler.end();
        match &mut self.bench {
            Some(run) => run.frame(
                world,
                &self.renderer,
                env.audio.stats(),
                dt * 1000.0,
                render,
            ),
            None => false,
        }
    }
}

/// The blur behind `screen`, if it has one: everything in `draw_list` so far
fn menu_blur(
    screen: &dyn crate::gui::screen::Screen,
    draw_list: &mut crate::renderer::quad::DrawList,
    has_background: bool,
) -> Option<Blur> {
    (has_background && screen.blurs_background()).then(|| Blur {
        radius: MENU_BACKGROUND_BLURRINESS,
        ui_split: draw_list.seal(),
    })
}
