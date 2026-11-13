//! The F3 debug overlay (vanilla `client/gui/components/DebugScreenOverlay`

use crate::gui::text::font::Font;
use crate::math::{section_coord, section_relative, wrap_degrees};
use crate::renderer::quad::DrawList;
use crate::renderer::{DeviceInfo, TextureId};
use glam::Vec3;

/// `DebugScreenOverlay.extractLines`: `int height = 9`.
const LINE_HEIGHT: f32 = 9.0;
/// `graphics.fill(..., -1873784752)` = `0x90505050`.
const BACKGROUND: [f32; 4] = [
    0x50 as f32 / 255.0,
    0x50 as f32 / 255.0,
    0x50 as f32 / 255.0,
    0x90 as f32 / 255.0,
];
/// `graphics.text(..., -2039584, false)` = `0xFFE0E0E0`, no shadow.
const TEXT: [f32; 4] = [
    0xE0 as f32 / 255.0,
    0xE0 as f32 / 255.0,
    0xE0 as f32 / 255.0,
    1.0,
];

/// Everything the implemented entries read off the running game. Assembled by
pub struct DebugContext<'a> {
    /// `Minecraft.getFps()`: whole frames in the last second, not `1 / dt`.
    pub fps: u32,
    /// The framerate limit in force (`None`: unlimited) and VSync.
    pub framerate_limit: Option<u32>,
    pub vsync: bool,
    /// The swapchain's present mode, e.g. `fifo`.
    pub present_mode: &'static str,
    /// Feet position (`Entity.getX/getY/getZ`), not the eye.
    pub position: Vec3,
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    /// `Window.getWidth()`/`getHeight()`, i.e. physical pixels.
    pub framebuffer: (u32, u32),
    pub device: &'a DeviceInfo,
    /// `Level.dimension().identifier()`, e.g. `minecraft:overworld`.
    pub dimension: &'a str,
    pub render: RenderStats,
    pub mesh: crate::world::level::MeshStats,
    /// Bytes of GPU mesh memory allocated and in use.
    pub mesh_memory: (u64, u64),
    /// [`crate::game::profiler::FrameProfiler::lines`].
    pub profile: Vec<String>,
    /// The GPU's time per pass ([`crate::renderer::Renderer::gpu_profile`]).
    pub gpu_profile: Vec<String>,
    /// The connection, sound, server and CPU per thread
    pub subsystems: Vec<String>,
    /// Overworld day count and ticks into the day (`DebugEntryDayCount`).
    pub day: i64,
    pub day_time: i64,
    pub extras: DebugExtras,
}

/// What the overlay shows besides what it computes itself: the entries that
#[derive(Default)]
pub struct DebugExtras {
    /// `DebugEntryTps`.
    pub server: Option<String>,
    /// `DebugEntrySimplePerformanceImpactors`.
    pub impactors: Vec<String>,
    /// The entries a player turns on (F3+F6), one group each.
    pub groups: Vec<Vec<String>>,
    /// `DebugEntryPlayerSpeed`, a line of the position group.
    pub speed: Option<String>,
    /// Biome, light and chunk source stats, in the stats block.
    pub regular: Vec<String>,
}

/// What the culling let through this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// Non-empty 16³ sections drawn and meshed, like vanilla's `C:`.
    pub sections_drawn: usize,
    pub sections_total: usize,
    /// Terrain vertices and draw calls (Melon's own line, `V:`).
    pub vertices_drawn: usize,
    pub terrain_draws: usize,
    pub entities_drawn: usize,
    pub entities_total: usize,
    /// Live particles drawn (`SQ`) and drawing nothing (`NR`, emitters).
    pub particles: (usize, usize),
}

/// `DebugScreenDisplayer`: the sink entries push their lines into.
#[derive(Default)]
struct DebugLines {
    priority: Vec<String>,
    regular: Vec<String>,
    /// One `Vec` per group, in insertion order (vanilla's `LinkedHashMap`).
    groups: Vec<Vec<String>>,
}

impl DebugLines {
    fn collect(context: &DebugContext<'_>) -> Self {
        let mut lines = Self::default();
        lines.game_version();
        lines.fps(context);
        lines.render_stats(context);
        lines.extras(context);
        lines.position(context);
        lines.time(context);
        lines.groups.extend(context.extras.groups.iter().cloned());
        lines.groups.push(context.profile.clone());
        lines.memory(context);
        lines.system_specs(context);
        lines.groups.push(context.gpu_profile.clone());
        if !context.subsystems.is_empty() {
            lines.groups.push(context.subsystems.clone());
        }
        lines
    }

    /// `DebugEntryVersion`: `Minecraft <version> (<launched>/<brand>)`.
    fn game_version(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        let minecraft = version.split_once("+mc").map_or("?", |(_, mc)| mc);
        self.priority
            .push(format!("Minecraft {minecraft} (melon {version}/melon)"));
    }

    /// `DebugEntryFps`: `%d fps T: %s%s` - the framerate limit (`inf`
    fn fps(&mut self, context: &DebugContext<'_>) {
        let limit = context
            .framerate_limit
            .map_or_else(|| "inf".to_owned(), |l| l.to_string());
        let vsync = if context.vsync { " vsync" } else { "" };
        self.priority.push(format!(
            "{} fps T: {limit}{vsync} ({})",
            context.fps, context.present_mode
        ));
    }

    /// `DebugEntryTps`, the performance impactors and the optional entries'
    fn extras(&mut self, context: &DebugContext<'_>) {
        let extras = &context.extras;
        self.regular.extend(extras.server.clone());
        self.regular.extend(extras.regular.iter().cloned());
        self.regular.extend(extras.impactors.iter().cloned());
    }

    /// `DebugEntryChunkRenderStats` and `DebugEntryEntityRenderStats`.
    fn render_stats(&mut self, context: &DebugContext<'_>) {
        let stats = context.render;
        self.regular.push(format!(
            "C: {}/{} pC: {:03} pM: {}",
            stats.sections_drawn,
            stats.sections_total,
            context.mesh.waiting,
            context.mesh.in_flight
        ));
        self.regular.push(format!(
            "V: {}k draws: {}",
            stats.vertices_drawn / 1000,
            stats.terrain_draws
        ));
        self.regular.push(format!(
            "E: {}/{}",
            stats.entities_drawn, stats.entities_total
        ));
        let (quads, hidden) = stats.particles;
        self.regular
            .push(format!("P: SQ {quads} NR {hidden} T {}", quads + hidden));
    }

    /// `DebugEntryPosition` + `DebugEntrySectionPosition`, which vanilla puts
    fn position(&mut self, context: &DebugContext<'_>) {
        let pos = context.position;
        let (bx, by, bz) = (
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        );
        let (cx, cz) = (section_coord(bx), section_coord(bz));
        let direction = facing(context.yaw_degrees);
        let mut group = vec![
            format!("XYZ: {:.3} / {:.5} / {:.3}", pos.x, pos.y, pos.z),
            format!("Block: {bx} {by} {bz}"),
            format!(
                "Chunk: {cx} {} {cz} [{} {} in r.{}.{}.mca]",
                section_coord(by),
                cx & 31,
                cz & 31,
                cx >> 5,
                cz >> 5
            ),
            format!(
                "Facing: {} ({}) ({:.1} / {:.1})",
                direction.0,
                direction.1,
                wrap_degrees(context.yaw_degrees),
                wrap_degrees(context.pitch_degrees)
            ),
            format!("{} FC: 0", context.dimension),
            format!(
                "Section-relative: {:02} {:02} {:02}",
                section_relative(bx),
                section_relative(by),
                section_relative(bz)
            ),
        ];
        group.extend(context.extras.speed.clone());
        self.groups.push(group);
    }

    /// `DebugEntryDayCount` plus the ticks into the current day.
    fn time(&mut self, context: &DebugContext<'_>) {
        self.groups.push(vec![
            format!("Day #{}", context.day),
            format!("Local time: {}", context.day_time),
        ]);
    }

    /// `DebugEntryMemory`, reduced to what a Rust process can answer - see
    fn memory(&mut self, context: &DebugContext<'_>) {
        let (allocated, used) = context.mesh_memory;
        self.groups.push(vec![
            match resident_set_bytes() {
                Some(bytes) => format!("Mem: {:03}MiB (RSS)", bytes / 1024 / 1024),
                None => "Mem: unknown".to_string(),
            },
            format!(
                "Meshes: {}/{}MiB (GPU)",
                used / 1024 / 1024,
                allocated / 1024 / 1024
            ),
        ]);
    }

    /// `DebugEntrySystemSpecs`.
    fn system_specs(&mut self, context: &DebugContext<'_>) {
        let device = context.device;
        self.groups.push(vec![
            format!("Rust: {}", env!("MELON_RUSTC")),
            format!("CPU: {}", cpu_name()),
            format!(
                "Display: {}x{} ({})",
                context.framebuffer.0, context.framebuffer.1, device.vendor
            ),
            format!("{}{}", device.name, device.type_suffix),
            format!("{} {}", device.backend, device.driver),
        ]);
    }
}

/// Toggled by F3 (`Options.keyDebugOverlay`). Owns nothing but that flag:
#[derive(Default)]
pub struct DebugOverlay {
    visible: bool,
}

impl DebugOverlay {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// `DebugScreenOverlay.render`'s column split, verbatim: priority lines
    fn columns(lines: DebugLines) -> (Vec<String>, Vec<String>) {
        let mut left: Vec<String> = Vec::new();
        let mut right: Vec<String> = Vec::new();

        for line in lines.priority {
            if left.len() > right.len() {
                right.push(line);
            } else {
                left.push(line);
            }
        }
        if !left.is_empty() {
            left.push(String::new());
        }
        if !right.is_empty() {
            right.push(String::new());
        }

        if !lines.regular.is_empty() {
            let mid = lines.regular.len().div_ceil(2);
            let mut regular = lines.regular;
            let tail = regular.split_off(mid);
            let had_tail = !tail.is_empty();
            left.extend(regular);
            right.extend(tail);
            left.push(String::new());
            if had_tail {
                right.push(String::new());
            }
        }

        let mid = lines.groups.len().div_ceil(2);
        for (i, group) in lines.groups.into_iter().enumerate() {
            if group.is_empty() {
                continue;
            }
            let column = if i < mid { &mut left } else { &mut right };
            column.extend(group);
            column.push(String::new());
        }

        (left, right)
    }

    pub fn draw(
        &self,
        draw_list: &mut DrawList,
        font: &Font,
        font_textures: &[TextureId],
        white: TextureId,
        context: &DebugContext<'_>,
        width: f32,
    ) {
        if !self.visible {
            return;
        }
        let (left, right) = Self::columns(DebugLines::collect(context));
        push_column(draw_list, font, font_textures, white, &left, width, true);
        push_column(draw_list, font, font_textures, white, &right, width, false);
    }
}

/// `DebugScreenOverlay.extractLines`: all backgrounds first, then all text,
fn push_column(
    draw_list: &mut DrawList,
    font: &Font,
    font_textures: &[TextureId],
    white: TextureId,
    lines: &[String],
    screen_width: f32,
    align_left: bool,
) {
    let placement = |line: &str, i: usize| {
        let text_width = font.text_width(line);
        let left = if align_left {
            2.0
        } else {
            screen_width - 2.0 - text_width
        };
        (left, 2.0 + LINE_HEIGHT * i as f32, text_width)
    };

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (left, top, text_width) = placement(line, i);
        draw_list.push_rect(
            white,
            left - 1.0,
            top - 1.0,
            text_width + 2.0,
            LINE_HEIGHT,
            [0.0, 0.0, 1.0, 1.0],
            BACKGROUND,
        );
    }
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (left, top, _) = placement(line, i);
        font.push_text(draw_list, font_textures, line, left.round(), top, TEXT);
    }
}

/// `Entity.getDirection()` (the horizontal facing yaw rounds to) plus
fn facing(yaw_degrees: f32) -> (&'static str, &'static str) {
    match ((yaw_degrees / 90.0 + 0.5).floor() as i32) & 3 {
        0 => ("south", "Towards positive Z"),
        1 => ("west", "Towards negative X"),
        2 => ("north", "Towards negative Z"),
        _ => ("east", "Towards positive X"),
    }
}

use crate::platform::resident_set_bytes;

/// `GLX._getCpuInfo()`'s equivalent. Read once, like vanilla: the overlay
fn cpu_name() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| crate::platform::cpu_name().unwrap_or_else(|| "unknown".to_string()))
}
