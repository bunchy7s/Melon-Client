//! `--bench`: the game plays itself through fixed phases in a fixed world

pub mod report;

use std::path::PathBuf;

use crate::audio::SoundInstance;
use crate::audio::source::SoundSource;
use crate::math::angle::{self, TAU};
use crate::world::player::{MoveInput, PlayerState};

/// The world the benchmark plays in, under `saves/`.
pub const WORLD_FOLDER: &str = "melon-bench";
/// Its seed, so every machine generates the same terrain.
pub const WORLD_SEED: &str = "melon-bench";

/// After this many ticks without new chunks or meshes, loading is done.
const SETTLE_TICKS: u32 = 40;
/// The share of the square of chunks around the player that loading waits
const EXPECTED_SHARE: f32 = 0.7;
/// Loading gives up waiting after this long (2.5 minutes).
const LOAD_TIMEOUT_TICKS: u32 = 3000;
/// How far from the player the `sound` phase's sounds are, in blocks.
const SOUND_DISTANCE: f64 = 6.0;
/// What the `sound` phase plays, in turn.
const SOUNDS: [&str; 5] = [
    "block.stone.break",
    "block.grass.step",
    "block.wood.place",
    "entity.experience_orb.pickup",
    "block.note_block.harp",
];

/// The command line's `--bench` options.
#[derive(Clone, Debug, Default)]
pub struct BenchOptions {
    /// Names the run's files; `bench` without `--label`.
    pub label: String,
    /// `--fps-cap N`: a framerate limit instead of none.
    pub fps_cap: Option<u32>,
    /// `--compare <summary.json>`: an earlier run to hold this one against.
    pub compare: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Load,
    Rise,
    Turn,
    Fly,
    Sound,
    Idle,
    Done,
}

impl Phase {
    pub const MEASURED: [Phase; 6] = [
        Phase::Load,
        Phase::Rise,
        Phase::Turn,
        Phase::Fly,
        Phase::Sound,
        Phase::Idle,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Phase::Load => "load",
            Phase::Rise => "rise",
            Phase::Turn => "turn",
            Phase::Fly => "fly",
            Phase::Sound => "sound",
            Phase::Idle => "idle",
            Phase::Done => "done",
        }
    }

    /// How long the phase runs; `None` for loading, which runs until the
    fn ticks(self) -> Option<u32> {
        match self {
            Phase::Load | Phase::Done => None,
            Phase::Rise => Some(40),
            Phase::Turn => Some(200),
            Phase::Fly => Some(400),
            Phase::Sound => Some(200),
            Phase::Idle => Some(100),
        }
    }

    fn next(self) -> Phase {
        match self {
            Phase::Load => Phase::Rise,
            Phase::Rise => Phase::Turn,
            Phase::Turn => Phase::Fly,
            Phase::Fly => Phase::Sound,
            Phase::Sound => Phase::Idle,
            Phase::Idle | Phase::Done => Phase::Done,
        }
    }
}

/// How far the world has come in, for when loading is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadState {
    pub chunks: usize,
    /// Sections waiting for a mesh or being meshed.
    pub meshing: usize,
}

pub struct BenchDriver {
    phase: Phase,
    /// Ticks into the phase.
    ticks: u32,
    /// The yaw the player had when loading was done; the turn and the
    yaw: f32,
    /// Ticks the [`LoadState`] stayed the same, while loading.
    settled: u32,
    last_load: LoadState,
    /// Loading waits for at least this many chunks.
    expected_chunks: usize,
}

impl BenchDriver {
    /// A run at this render distance, which says how many chunks loading
    pub fn new(render_distance: u8) -> Self {
        let side = f32::from(render_distance) * 2.0 + 1.0;
        Self {
            phase: Phase::Load,
            ticks: 0,
            yaw: 0.0,
            settled: 0,
            last_load: LoadState::default(),
            expected_chunks: (side * side * EXPECTED_SHARE) as usize,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// One tick: turns `player` and returns the keys held, and the sound to
    pub fn tick(
        &mut self,
        player: &mut PlayerState,
        load: LoadState,
    ) -> (MoveInput, Option<SoundInstance<'static>>) {
        self.ticks += 1;
        let done = match self.phase.ticks() {
            Some(ticks) => self.ticks >= ticks,
            None => self.phase == Phase::Load && self.loaded(load),
        };
        if done {
            self.phase = self.phase.next();
            self.ticks = 0;
            if self.phase == Phase::Rise {
                self.yaw = player.yaw();
            }
        }
        let t = self.ticks as f32 / self.phase.ticks().unwrap_or(1) as f32;
        let mut input = MoveInput::default();
        let mut sound = None;
        match self.phase {
            Phase::Load | Phase::Done => {}
            Phase::Rise => {
                if self.ticks == 0 {
                    player.start_flying();
                }
                player.set_rotation(self.yaw, 0.0);
                input.jump = true;
            }
            Phase::Turn => {
                let pitch = 30.0 * angle::sin(t * TAU * 2.0);
                player.set_rotation(self.yaw + 360.0 * t, pitch);
            }
            Phase::Fly => {
                player.set_rotation(self.yaw, 0.0);
                input.forward = 1.0;
                input.sprint = true;
            }
            Phase::Sound => {
                let event = SOUNDS[self.ticks as usize % SOUNDS.len()];
                let turn = f64::from(self.ticks) * 0.7;
                let feet = player.feet();
                let at = [
                    feet.x + turn.cos() * SOUND_DISTANCE,
                    feet.y + 1.0,
                    feet.z + turn.sin() * SOUND_DISTANCE,
                ];
                sound = Some(
                    SoundInstance::of(event)
                        .on(SoundSource::Blocks)
                        .volume(0.5)
                        .at(at),
                );
            }
            Phase::Idle => {}
        }
        (input, sound)
    }

    /// Loading is done once the chunks around have come and nothing
    fn loaded(&mut self, load: LoadState) -> bool {
        let enough = load.chunks >= self.expected_chunks.max(1);
        if load == self.last_load && enough && load.meshing == 0 {
            self.settled += 1;
        } else {
            self.settled = 0;
        }
        self.last_load = load;
        self.settled >= SETTLE_TICKS || self.ticks >= LOAD_TIMEOUT_TICKS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn run_until(driver: &mut BenchDriver, player: &mut PlayerState, phase: Phase) -> u32 {
        let load = LoadState {
            chunks: 9,
            meshing: 0,
        };
        let mut ticks = 0;
        while driver.phase() != phase {
            driver.tick(player, load);
            ticks += 1;
            assert!(ticks < 10_000, "stuck in {:?}", driver.phase());
        }
        ticks
    }

    #[test]
    fn phases_run_in_order_for_their_ticks() {
        let mut driver = BenchDriver::new(1);
        let mut player = PlayerState::new(DVec3::ZERO);
        assert_eq!(
            run_until(&mut driver, &mut player, Phase::Rise),
            SETTLE_TICKS + 1
        );
        assert_eq!(run_until(&mut driver, &mut player, Phase::Turn), 40);
        assert_eq!(run_until(&mut driver, &mut player, Phase::Fly), 200);
        assert_eq!(run_until(&mut driver, &mut player, Phase::Sound), 400);
        assert_eq!(run_until(&mut driver, &mut player, Phase::Idle), 200);
        assert_eq!(run_until(&mut driver, &mut player, Phase::Done), 100);
    }

    #[test]
    fn loading_waits_while_chunks_keep_coming() {
        let mut driver = BenchDriver::new(1);
        let mut player = PlayerState::new(DVec3::ZERO);
        for chunks in 1..100 {
            let load = LoadState { chunks, meshing: 0 };
            driver.tick(&mut player, load);
        }
        assert_eq!(driver.phase(), Phase::Load);
    }

    #[test]
    fn flying_holds_forward_and_sprint() {
        let mut driver = BenchDriver::new(1);
        let mut player = PlayerState::new(DVec3::ZERO);
        run_until(&mut driver, &mut player, Phase::Fly);
        let (input, sound) = driver.tick(&mut player, LoadState::default());
        assert_eq!(input.forward, 1.0);
        assert!(input.sprint);
        assert!(sound.is_none());
        run_until(&mut driver, &mut player, Phase::Sound);
        let (_, sound) = driver.tick(&mut player, LoadState::default());
        assert!(sound.is_some_and(|s| s.position.is_some()));
    }
}
