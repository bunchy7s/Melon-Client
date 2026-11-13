//! The sounds the client makes up itself, with no packet asking for them:

use rand::RngExt;

use super::InWorld;
use super::controls::WorldContext;
use crate::audio::SoundInstance;
use crate::audio::source::SoundSource;
use crate::world::physics::eye_in_water;

/// `UnderwaterAmbientSoundHandler`'s chances per tick.
const ULTRA_RARE: f32 = 1.0e-4;
const RARE: f32 = 1.0e-3;
const COMMON: f32 = 1.0e-2;

/// What the ambient sounds carry from one tick to the next.
#[derive(Default)]
pub(super) struct Ambient {
    /// `LocalPlayer.wasUnderwater`.
    under_water: bool,
    /// `UnderwaterAmbientSoundHandler.tickDelay`.
    delay: i32,
    /// `ClientLevel.rainSoundTime`.
    rain_time: i32,
}

/// Which of the underwater sounds a roll of `chance` (`0..1`) picks.
fn underwater_addition(chance: f32) -> Option<&'static str> {
    if chance < ULTRA_RARE {
        Some("ambient.underwater.loop.additions.ultra_rare")
    } else if chance < RARE {
        Some("ambient.underwater.loop.additions.rare")
    } else if chance < COMMON {
        Some("ambient.underwater.loop.additions")
    } else {
        None
    }
}

impl InWorld {
    pub(super) fn tick_ambient_sounds(&mut self, ui: &WorldContext) {
        self.tick_underwater_sounds();
        self.tick_rain_sounds(i32::from(ui.config.weather_radius));
    }

    fn ambient_sound(
        &mut self,
        event: &'static str,
        at: [f64; 3],
        source: SoundSource,
        volume: f32,
        pitch: f32,
    ) {
        let mut sound = SoundInstance::of(event).at(at);
        sound.source = source;
        sound.volume = volume;
        sound.pitch = pitch;
        self.sounds.push(sound);
    }

    fn tick_underwater_sounds(&mut self) {
        let eye = self.player.tick_eye();
        let at = [eye.x, eye.y, eye.z];
        let now = eye_in_water(&self.level, eye.as_vec3());
        let spectator = self.game_mode.current.is_spectator();
        let was = self.ambient.under_water;
        if !spectator {
            if now && !was {
                self.ambient_sound(
                    "ambient.underwater.enter",
                    at,
                    SoundSource::Ambient,
                    1.0,
                    1.0,
                );
            } else if was && !now {
                self.ambient_sound(
                    "ambient.underwater.exit",
                    at,
                    SoundSource::Ambient,
                    1.0,
                    1.0,
                );
            }
            self.ambient.under_water = now;
        }
        self.ambient.delay -= 1;
        if self.ambient.delay <= 0
            && self.ambient.under_water
            && let Some(event) = underwater_addition(rand::random::<f32>())
        {
            self.ambient.delay = 0;
            self.ambient_sound(event, at, SoundSource::Ambient, 1.0, 1.0);
        }
    }

    /// `ClientLevel.tickRain`'s sound: a patch of rain somewhere near,
    fn tick_rain_sounds(&mut self, radius: i32) {
        let rain = self.weather.rain;
        if rain <= 0.0 || radius <= 0 {
            return;
        }
        let camera = self.player.tick_eye().floor().as_ivec3();
        let diameter = 2 * radius + 1;
        let drops = ((0.225 * (diameter * diameter) as f32 * rain * rain) as i32).max(0);
        let mut random = rand::rng();
        let mut spot = None;
        for _ in 0..drops {
            let x = camera.x + random.random_range(0..diameter) - radius;
            let z = camera.z + random.random_range(0..diameter) - radius;
            let y = self.level.motion_blocking_height(x, z);
            let rains = self
                .level
                .biome(x, y, z)
                .and_then(|id| self.biomes.biome(id))
                .is_some_and(|biome| {
                    biome.precipitation() == crate::world::weather::Precipitation::Rain
                });
            if y > self.level.height().0 && (y - camera.y).abs() <= 10 && rains {
                spot = Some([x, y - 1, z]);
            }
        }
        let Some(spot) = spot else {
            return;
        };
        let wait = self.ambient.rain_time;
        self.ambient.rain_time += 1;
        if random.random_range(0..3) >= wait {
            return;
        }
        self.ambient.rain_time = 0;
        let covered = self.level.motion_blocking_height(camera.x, camera.z) > camera.y;
        let at = [
            f64::from(spot[0]) + 0.5,
            f64::from(spot[1]) + 0.5,
            f64::from(spot[2]) + 0.5,
        ];
        if spot[1] > camera.y + 1 && covered {
            self.ambient_sound("weather.rain.above", at, SoundSource::Weather, 0.1, 0.5);
        } else {
            self.ambient_sound("weather.rain", at, SoundSource::Weather, 0.2, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rarer_the_roll_the_rarer_the_sound() {
        assert_eq!(
            underwater_addition(0.00001),
            Some("ambient.underwater.loop.additions.ultra_rare")
        );
        assert_eq!(
            underwater_addition(0.0005),
            Some("ambient.underwater.loop.additions.rare")
        );
        assert_eq!(
            underwater_addition(0.005),
            Some("ambient.underwater.loop.additions")
        );
        assert_eq!(underwater_addition(0.5), None);
    }
}
