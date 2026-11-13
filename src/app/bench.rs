//! `--bench` in the app: the fixed world it joins, the options it holds

use std::path::{Path, PathBuf};

use crate::audio::stats::AudioStats;
use crate::game::InWorld;
use crate::game::bench::report::{self, FrameSample, Recorder, Setup};
use crate::game::bench::{BenchOptions, Phase, Scene, WORLD_SEED};
use crate::game::subsystems::Snapshot;
use crate::gui::hud::debug::RenderStats;
use crate::join::Target;
use crate::renderer::Renderer;
use crate::settings::options::{self, Config};
use crate::singleplayer::{
    self, Difficulty, WorldCreationMode, WorldSettings, rules, rules::Value,
};

/// Where the runs' CSV and summaries go without `--out`, from the working
/// directory.
const OUT_DIR: &str = "target/melon-bench";

pub(super) struct BenchRun {
    pub options: BenchOptions,
    recorder: Option<Recorder>,
    phase: Option<Phase>,
}

impl BenchRun {
    pub fn new(options: BenchOptions) -> Self {
        Self {
            options,
            recorder: None,
            phase: None,
        }
    }

    /// The world was entered: recording starts.
    pub fn start(&mut self, renderer: &Renderer, config: &Config) {
        let device = renderer.device_info();
        self.recorder = Some(Recorder::new(Setup {
            label: self.options.label.clone(),
            scene: self.options.scene.name().to_owned(),
            device: format!("{}{}", device.name, device.type_suffix),
            resolution: renderer.framebuffer_size(),
            render_distance: config.render_distance,
            fps_cap: self.options.fps_cap,
            present_mode: renderer.present_mode_name().to_owned(),
        }));
        println!(
            "Bench '{}' ({}): {}{}, {}x{}, Sichtweite {}, FPS-Grenze {}",
            self.options.label,
            self.options.scene.name(),
            device.name,
            device.type_suffix,
            renderer.framebuffer_size().0,
            renderer.framebuffer_size().1,
            config.render_distance,
            self.options
                .fps_cap
                .map_or_else(|| "keine".to_owned(), |fps| fps.to_string()),
        );
    }

    /// One frame drawn, `dt_ms` after the last. Whether the run is done.
    pub fn frame(
        &mut self,
        world: &InWorld,
        renderer: &Renderer,
        audio: &AudioStats,
        dt_ms: f32,
        render: RenderStats,
    ) -> bool {
        let (Some(recorder), Some(phase)) = (&mut self.recorder, world.bench_phase()) else {
            return false;
        };
        let net = world.net_stats();
        if self.phase != Some(phase) {
            recorder.enter(phase, Snapshot::take(net, audio));
            self.phase = Some(phase);
            println!(
                "Bench: Phase {} ({} Entities)",
                phase.name(),
                world.entity_count()
            );
        }
        if phase == Phase::Done {
            return true;
        }
        let (cpu_ms, cpu) = world.profiler.last_frame();
        let (gpu_ms, gpu) = renderer.gpu_last_frame();
        recorder.frame(
            FrameSample {
                frame_ms: dt_ms,
                cpu_ms,
                gpu_ms,
                cpu: cpu.to_vec(),
                gpu: gpu.to_vec(),
                terrain_draws: render.terrain_draws,
                sections_drawn: render.sections_drawn,
                entities: world.entity_count(),
            },
            world.server_mspt(),
            net.totals().backlog,
            renderer.mesh_memory().1,
            world.chunk_count(),
        );
        false
    }

    /// Writes the files and prints the summary, and the comparison when
    pub fn finish(&self) {
        let Some(recorder) = &self.recorder else {
            eprintln!("Bench: die Welt wurde nie betreten, nichts gemessen");
            return;
        };
        let summary = recorder.summary();
        let dir = self.options.out.as_deref().unwrap_or(Path::new(OUT_DIR));
        match recorder.write(dir) {
            Ok(path) => println!("Bench: {} (und .csv)", path.display()),
            Err(e) => eprintln!("Bench: konnte nicht schreiben: {e:#}"),
        }
        print!("{}", report::table(&summary));
        if let Some(path) = &self.options.compare {
            match std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
            {
                Some(old) => {
                    println!("Vergleich mit {}:", path.display());
                    print!("{}", report::compare(&old, &summary));
                }
                None => eprintln!("Bench: {} nicht lesbar", path.display()),
            }
        }
    }
}

/// What the benchmark holds still for the run, in memory only: no VSync,
pub(super) fn override_config(config: &mut Config) {
    config.vsync = false;
    config.max_fps = options::MAX_FPS_RANGE.1;
}

/// The benchmark's world, made the first time: creative, fixed seed, no
pub(super) fn prepare_world(game_dir: &Path, scene: Scene) -> anyhow::Result<Target> {
    let folder = scene.world_folder();
    let active = scene == Scene::Active;
    if active {
        // Made anew: what the last run summoned would still be there.
        let old = singleplayer::world_dir(game_dir, folder);
        if old.is_dir() {
            std::fs::remove_dir_all(&old)?;
        }
    }
    let settings = WorldSettings {
        name: "Melon Bench".to_owned(),
        game_mode: WorldCreationMode::Creative,
        seed: WORLD_SEED.to_owned(),
        // Monsters stay in the active world (and burn in its daylight).
        difficulty: if active {
            Difficulty::Normal
        } else {
            Difficulty::Peaceful
        },
        allow_cheats: true,
        ..WorldSettings::default()
    };
    singleplayer::save_world(game_dir, folder, &settings)?;
    let world_dir = singleplayer::world_dir(game_dir, folder);
    // The active scene lets the time run into the dusk; the weather it sets
    // itself, and only its own mobs come, so runs stay comparable.
    let rules: rules::Changes = [
        ("advance_time", Value::Bool(active)),
        ("advance_weather", Value::Bool(false)),
        ("spawn_mobs", Value::Bool(false)),
        ("send_command_feedback", Value::Bool(false)),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    rules::write_changes(&world_dir, &rules)?;
    let players: PathBuf = world_dir.join("players").join("data");
    if players.is_dir() {
        std::fs::remove_dir_all(&players)?;
    }
    Ok(Target::World {
        save_path: world_dir.clone(),
        game_dir: game_dir.to_path_buf(),
        settings,
    })
}
