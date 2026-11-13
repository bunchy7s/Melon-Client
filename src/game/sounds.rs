//! The sounds of the world: the player's own (breaking, placing and hitting

use super::{InWorld, minecraft_yaw};
use crate::audio::SoundInstance;
use crate::audio::block_sounds::{BlockSound, SoundType};
use crate::audio::level_sounds::LevelSound;
use crate::audio::source::SoundSource;
use crate::network::play::game::sound::{BREAK_BLOCK, Explosion, LevelEvent, ServerSound, SoundAt};
use pumpkin_data::{Block, BlockStateId};
use std::time::{Duration, Instant};

/// `Entity.moveDist` grows by this much per block walked.
const STEP_FACTOR: f64 = 0.6;
/// How far below the feet the block being walked on is (`getOnPos`).
const ON_POS_OFFSET: f64 = 0.2;
/// How far from the camera a global level event is heard.
const GLOBAL_DISTANCE: f64 = 2.0;
/// A server sound this close to one the client just played itself is the
const ECHO_RANGE: f64 = 1.5;
const ECHO_WINDOW: Duration = Duration::from_millis(750);

/// `Entity.moveDist`, `nextStep` and `fallDistance`.
#[derive(Default)]
pub(super) struct Footsteps {
    last: Option<[f64; 3]>,
    move_dist: f64,
    next_step: f64,
    fall_distance: f64,
}

/// What the client played itself lately: a server that tells every player
#[derive(Default)]
pub(super) struct Echoes {
    recent: Vec<(String, [f64; 3], Instant)>,
}

impl Echoes {
    fn remember(&mut self, sound: &SoundInstance, at: [f64; 3]) {
        let now = Instant::now();
        self.recent
            .retain(|(_, _, when)| now.duration_since(*when) < ECHO_WINDOW);
        self.recent.push((sound.event.to_string(), at, now));
    }

    /// Whether `event` at `at` is one the client just played.
    fn heard(&self, event: &str, at: [f64; 3]) -> bool {
        let now = Instant::now();
        self.recent.iter().any(|(name, place, when)| {
            name == event
                && now.duration_since(*when) < ECHO_WINDOW
                && distance(*place, at) < ECHO_RANGE
        })
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The middle of a block.
pub(super) fn block_center(pos: [i32; 3]) -> [f64; 3] {
    pos.map(|c| f64::from(c) + 0.5)
}

/// The level events whose sound depends on their data, not only the event.
fn special_level_sound(event: LevelEvent, at: [f64; 3]) -> Option<SoundInstance<'static>> {
    let data = event.data;
    let r = rand::random::<f32>;
    let sound = |name, source, volume: f32, pitch: f32| {
        SoundInstance::of(name)
            .on(source)
            .volume(volume)
            .pitch(pitch)
            .at(at)
    };
    Some(match event.event {
        1009 => match data {
            0 => sound(
                "block.fire.extinguish",
                SoundSource::Blocks,
                0.5,
                2.6 + (r() - r()) * 0.8,
            ),
            1 => sound(
                "entity.generic.extinguish_fire",
                SoundSource::Blocks,
                0.7,
                1.6 + (r() - r()) * 0.4,
            ),
            _ => return None,
        },
        1500 => sound(
            if data > 0 {
                "block.composter.fill_success"
            } else {
                "block.composter.fill"
            },
            SoundSource::Blocks,
            1.0,
            1.0,
        ),
        3006 => {
            let count = (data >> 6) as f32;
            if count <= 0.0 {
                sound("block.sculk.charge", SoundSource::Blocks, 1.0, 1.0)
            } else if r() < 0.3 + count * 0.1 {
                let volume = 0.15 + 0.02 * count * count * r();
                sound(
                    "block.sculk.charge",
                    SoundSource::Blocks,
                    volume,
                    0.4 + 0.3 * count * r(),
                )
            } else {
                return None;
            }
        }
        3008 => {
            let name = match block_name(u16::try_from(data).unwrap_or(0)) {
                "suspicious_sand" => "item.brush.brushing.sand.complete",
                "suspicious_gravel" => "item.brush.brushing.gravel.complete",
                _ => return None,
            };
            sound(name, SoundSource::Players, 1.0, 1.0)
        }
        3020 => sound(
            "block.trial_spawner.ominous_activate",
            SoundSource::Blocks,
            if data == 0 { 0.3 } else { 1.0 },
            1.0 + (r() - r()) * 0.2,
        ),
        _ => return None,
    })
}

/// Blocks that spawn no destroy event when a player breaks them
pub(super) fn breaks_silently(state: u16) -> bool {
    matches!(block_name(state), "fire" | "soul_fire")
}

/// The block name of a state, `air` for one that doesn't exist.
fn block_name(state: u16) -> &'static str {
    BlockStateId::new(state).map_or("air", |id| Block::from_state_id(id).name)
}

impl InWorld {
    /// The sounds queued since the last call.
    pub fn take_sounds(&mut self) -> Vec<SoundInstance<'static>> {
        std::mem::take(&mut self.sounds)
    }

    /// Where the player's ears are and which way they face, for the sounds
    pub fn listener(&self) -> ([f64; 3], f32) {
        let eye = self.player.tick_eye();
        ([eye.x, eye.y, eye.z], minecraft_yaw(self.player.yaw()))
    }

    /// Queues what block `state` sounds like for `kind`, heard from `at`.
    pub(super) fn queue_block_sound(&mut self, state: u16, kind: BlockSound, at: [f64; 3]) {
        if let Some(sound) = SoundType::of(block_name(state)).sound(kind) {
            let sound = sound.at(at);
            self.echoes.remember(&sound, at);
            self.sounds.push(sound);
        }
    }

    /// `Consumable.onUseTick`: the bite or sip of the food or drink being
    pub(super) fn play_consume_sound(&mut self) {
        let Some(hand) = self.hands.consuming.take() else {
            return;
        };
        let stack = self.hand_stack(hand);
        if super::particles::has_consume_particles(stack) {
            let item = stack.item.id;
            self.eating_particles(item, 5);
        }
        let Some((event, volume, pitch)) =
            crate::world::player::hands::consume_sound(self.hand_stack(hand), rand::random::<f32>)
        else {
            return;
        };
        let eye = self.player.tick_eye();
        let mut sound = SoundInstance::of(event).at([eye.x, eye.y, eye.z]);
        sound.source = SoundSource::Players;
        sound.volume = volume;
        sound.pitch = pitch;
        self.sounds.push(sound);
    }

    /// `ClientboundSoundPacket` and `ClientboundSoundEntityPacket`.
    pub(super) fn on_server_sound(&mut self, sound: &ServerSound) {
        let at = match sound.at {
            SoundAt::Position(at) => at,
            SoundAt::Entity(id) if id == self.joined.session.info.entity_id => {
                let feet = self.player.feet();
                [feet.x, feet.y, feet.z]
            }
            SoundAt::Entity(id) => match self.entities.get(id) {
                Some(entity) => entity.tick_pos().to_array(),
                None => return,
            },
        };
        if self.echoes.heard(&sound.event, at) {
            return;
        }
        self.sounds.push(
            SoundInstance::of(sound.event.clone())
                .on(sound.source)
                .volume(sound.volume)
                .pitch(sound.pitch)
                .at(at),
        );
    }

    /// `ClientHandler.handleExplosion`: the blast sound, at volume 4 and a
    pub(super) fn on_explosion(&mut self, explosion: &Explosion) {
        let Some(sound) = &explosion.sound else {
            return;
        };
        let jitter = rand::random::<f32>() - rand::random::<f32>();
        self.sounds.push(
            SoundInstance::of(sound.clone())
                .on(SoundSource::Blocks)
                .volume(4.0)
                .pitch((1.0 + jitter * 0.2) * 0.7)
                .at(explosion.center),
        );
    }

    /// `ClientboundLevelEventPacket`: `LevelEventHandler.levelEvent`'s
    pub(super) fn on_level_event(&mut self, event: LevelEvent) {
        let at = block_center(event.pos);
        let sound = if event.event == BREAK_BLOCK {
            let state = u16::try_from(event.data).unwrap_or(0);
            let Some(sound) = SoundType::of(block_name(state)).sound(BlockSound::Break) else {
                return;
            };
            if self.echoes.heard(&sound.event, at) {
                return;
            }
            if event.global { sound } else { sound.at(at) }
        } else if let Some(sound) = LevelSound::of(event.event) {
            if event.global {
                sound.instance(self.toward(at, GLOBAL_DISTANCE))
            } else {
                sound.instance(at)
            }
        } else if let Some(sound) = special_level_sound(event, at) {
            sound
        } else {
            return;
        };
        self.sounds.push(sound);
    }

    /// The point `distance` blocks from the eye toward `target`.
    fn toward(&self, target: [f64; 3], distance: f64) -> [f64; 3] {
        let eye = self.player.tick_eye();
        let eye = [eye.x, eye.y, eye.z];
        let delta = [0, 1, 2].map(|i| target[i] - eye[i]);
        let length = delta.iter().map(|d| d * d).sum::<f64>().sqrt().max(1e-9);
        [0, 1, 2].map(|i| eye[i] + delta[i] / length * distance)
    }

    /// `Entity.move`'s step and landing sounds, after the player moved.
    pub(super) fn tick_footsteps(&mut self) {
        let feet = self.player.feet();
        let now = [feet.x, feet.y, feet.z];
        let Some(last) = self.footsteps.last.replace(now) else {
            return;
        };
        let steps = &mut self.footsteps;
        let on_ground = self.player.on_ground();
        if !on_ground {
            if now[1] < last[1] {
                steps.fall_distance += last[1] - now[1];
            }
            return;
        }
        let below = self.block_below_feet(ON_POS_OFFSET);
        let landed = std::mem::take(&mut self.footsteps.fall_distance) > 0.0;
        if landed {
            self.queue_block_sound(below, BlockSound::Fall, now);
            self.footsteps.next_step = self.footsteps.move_dist.floor() + 1.0;
        }
        if self.keys.sneak || self.player.flying() {
            return;
        }
        let walked = (now[0] - last[0]).hypot(now[2] - last[2]);
        self.footsteps.move_dist += walked * STEP_FACTOR;
        if self.footsteps.move_dist > self.footsteps.next_step {
            self.footsteps.next_step = self.footsteps.move_dist.floor() + 1.0;
            self.queue_block_sound(below, BlockSound::Step, now);
        }
    }

    fn block_below_feet(&self, offset: f64) -> u16 {
        let feet = self.player.feet();
        self.level
            .block_state(
                feet.x.floor() as i32,
                (feet.y - offset).floor() as i32,
                feet.z.floor() as i32,
            )
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_sound_next_to_one_just_played_is_an_echo() {
        let mut echoes = Echoes::default();
        let played = SoundInstance::of("block.stone.break");
        echoes.remember(&played, [10.5, 64.5, 3.5]);
        assert!(echoes.heard("block.stone.break", [10.5, 65.0, 3.5]));
        assert!(!echoes.heard("block.stone.break", [20.5, 64.5, 3.5]));
        assert!(!echoes.heard("block.stone.place", [10.5, 64.5, 3.5]));
    }

    #[test]
    fn fire_breaks_without_a_sound() {
        assert!(breaks_silently(Block::FIRE.default_state.id.as_u16()));
        assert!(breaks_silently(Block::SOUL_FIRE.default_state.id.as_u16()));
        assert!(!breaks_silently(Block::STONE.default_state.id.as_u16()));
    }

    #[test]
    fn a_block_is_heard_from_its_middle() {
        assert_eq!(block_center([1, 2, -3]), [1.5, 2.5, -2.5]);
    }
}
