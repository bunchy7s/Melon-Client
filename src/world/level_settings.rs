//! What the server says about the level besides its blocks and border:

/// `ClientboundSetDefaultSpawnPositionPacket`: where the world's spawn is.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPoint {
    pub dimension: String,
    pub pos: [i32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// One packet's worth of it.
#[derive(Clone, Debug, PartialEq)]
pub enum LevelSetting {
    /// `ClientboundChangeDifficultyPacket`: peaceful 0 to hard 3.
    Difficulty {
        difficulty: u8,
        locked: bool,
    },
    DefaultSpawn(SpawnPoint),
    /// `ClientboundSetChunkCacheCenterPacket`: the chunk the view is around.
    ChunkCenter {
        x: i32,
        z: i32,
    },
    /// `ClientboundSetChunkCacheRadiusPacket`: the server's view distance.
    ChunkRadius(i32),
    SimulationDistance(i32),
    /// `ClientboundTickingStatePacket`: ticks per second, and `/tick freeze`.
    TickingState {
        rate: f32,
        frozen: bool,
    },
    /// `ClientboundTickingStepPacket`: ticks to run while frozen.
    TickingStep(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LevelSettings {
    pub difficulty: u8,
    pub difficulty_locked: bool,
    pub spawn: Option<SpawnPoint>,
    pub chunk_center: (i32, i32),
    pub server_view_distance: Option<i32>,
    pub simulation_distance: Option<i32>,
    pub tick_rate: f32,
    pub frozen: bool,
    /// Ticks still to run while frozen.
    pub frozen_steps: i32,
}

impl Default for LevelSettings {
    fn default() -> Self {
        Self {
            difficulty: 2,
            difficulty_locked: false,
            spawn: None,
            chunk_center: (0, 0),
            server_view_distance: None,
            simulation_distance: None,
            tick_rate: 20.0,
            frozen: false,
            frozen_steps: 0,
        }
    }
}

impl LevelSettings {
    pub fn apply(&mut self, setting: LevelSetting) {
        match setting {
            LevelSetting::Difficulty { difficulty, locked } => {
                self.difficulty = difficulty;
                self.difficulty_locked = locked;
            }
            LevelSetting::DefaultSpawn(spawn) => self.spawn = Some(spawn),
            LevelSetting::ChunkCenter { x, z } => self.chunk_center = (x, z),
            LevelSetting::ChunkRadius(radius) => self.server_view_distance = Some(radius),
            LevelSetting::SimulationDistance(distance) => {
                self.simulation_distance = Some(distance);
            }
            LevelSetting::TickingState { rate, frozen } => {
                self.tick_rate = rate;
                self.frozen = frozen;
            }
            LevelSetting::TickingStep(steps) => self.frozen_steps = steps.max(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_packet_sets_its_part() {
        let mut settings = LevelSettings::default();
        settings.apply(LevelSetting::TickingState {
            rate: 5.0,
            frozen: true,
        });
        settings.apply(LevelSetting::Difficulty {
            difficulty: 3,
            locked: true,
        });
        settings.apply(LevelSetting::ChunkCenter { x: -2, z: 7 });
        assert_eq!((settings.tick_rate, settings.frozen), (5.0, true));
        assert_eq!((settings.difficulty, settings.difficulty_locked), (3, true));
        assert_eq!(settings.chunk_center, (-2, 7));
    }
}
