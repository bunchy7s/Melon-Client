//! Where a sound is: how far it carries and which ear it is nearer to.

use crate::math::angle::{self, QUARTER_PI};
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate};
use rodio::{Sample, Source};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Frames between two looks at where the listener is: about 5 ms.
const REFRESH_FRAMES: u32 = 256;

/// Where the player's ears are, written by the game each frame and read by
#[derive(Default)]
pub struct Listener {
    position: [AtomicU32; 3],
    /// The horizontal direction to the listener's right.
    right: [AtomicU32; 2],
}

impl Listener {
    /// The listener at `position` looking along `yaw_degrees`
    pub fn set(&self, position: [f64; 3], yaw_degrees: f32) {
        for (slot, value) in self.position.iter().zip(position) {
            slot.store((value as f32).to_bits(), Ordering::Relaxed);
        }
        let yaw = yaw_degrees.to_radians();
        self.right[0].store((-angle::cos(yaw)).to_bits(), Ordering::Relaxed);
        self.right[1].store((-angle::sin(yaw)).to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> ([f32; 3], [f32; 2]) {
        let load = |slot: &AtomicU32| f32::from_bits(slot.load(Ordering::Relaxed));
        (
            [
                load(&self.position[0]),
                load(&self.position[1]),
                load(&self.position[2]),
            ],
            [load(&self.right[0]), load(&self.right[1])],
        )
    }
}

/// `AL_LINEAR_DISTANCE_CLAMPED`: full volume at the listener, none from
pub fn linear_gain(distance: f32, max_distance: f32) -> f32 {
    if max_distance <= 0.0 {
        return 0.0;
    }
    (1.0 - distance / max_distance).clamp(0.0, 1.0)
}

/// The volume of each channel for a sound at `emitter`: its distance gain,
pub fn channel_gains(
    listener: [f32; 3],
    right: [f32; 2],
    emitter: [f32; 3],
    max_distance: f32,
) -> (f32, f32) {
    let delta = [
        emitter[0] - listener[0],
        emitter[1] - listener[1],
        emitter[2] - listener[2],
    ];
    let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    let gain = linear_gain(distance, max_distance);
    let horizontal = delta[0].hypot(delta[2]);
    let side = if horizontal < 1.0e-3 {
        0.0
    } else {
        ((delta[0] * right[0] + delta[2] * right[1]) / horizontal).clamp(-1.0, 1.0)
    };
    let angle = (side + 1.0) * QUARTER_PI;
    let boost = crate::math::SQRT_2;
    (
        gain * (boost * angle::cos(angle)).min(1.0),
        gain * (boost * angle::sin(angle)).min(1.0),
    )
}

/// A stereo source heard from a place: its two channels scaled by
pub struct Spatial<I> {
    input: I,
    listener: Arc<Listener>,
    emitter: [f32; 3],
    max_distance: f32,
    gains: (f32, f32),
    /// Which channel the next sample belongs to.
    channel: usize,
    frames_left: u32,
}

impl<I: Source> Spatial<I> {
    /// `input` must have two channels.
    pub fn new(input: I, listener: Arc<Listener>, emitter: [f64; 3], max_distance: f32) -> Self {
        debug_assert_eq!(input.channels().get(), 2);
        let mut spatial = Self {
            input,
            listener,
            emitter: [emitter[0] as f32, emitter[1] as f32, emitter[2] as f32],
            max_distance,
            gains: (0.0, 0.0),
            channel: 0,
            frames_left: 0,
        };
        spatial.refresh();
        spatial
    }

    fn refresh(&mut self) {
        let (position, right) = self.listener.get();
        self.gains = channel_gains(position, right, self.emitter, self.max_distance);
        self.frames_left = REFRESH_FRAMES;
    }
}

impl<I: Source> Iterator for Spatial<I> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.channel == 0 && self.frames_left == 0 {
            self.refresh();
        }
        let sample = self.input.next()?;
        let gain = if self.channel == 0 {
            self.gains.0
        } else {
            self.gains.1
        };
        self.channel ^= 1;
        if self.channel == 0 {
            self.frames_left -= 1;
        }
        Some(sample * gain)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<I: Source> Source for Spatial<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.input.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    #[test]
    fn a_sound_fades_linearly_to_nothing_at_its_range() {
        assert_eq!(linear_gain(0.0, 16.0), 1.0);
        assert_eq!(linear_gain(8.0, 16.0), 0.5);
        assert_eq!(linear_gain(16.0, 16.0), 0.0);
        assert_eq!(linear_gain(40.0, 16.0), 0.0);
        assert_eq!(linear_gain(1.0, 0.0), 0.0);
    }

    /// Facing south (yaw 0): the right hand is -x (west).
    #[test]
    fn a_sound_to_the_right_is_louder_in_the_right_channel() {
        let listener = Listener::default();
        listener.set([0.0; 3], 0.0);
        let (position, right) = listener.get();
        assert!((right[0] + 1.0).abs() < 1e-6 && right[1].abs() < 1e-6);
        let (l, r) = channel_gains(position, right, [-4.0, 0.0, 0.0], 16.0);
        assert!(l < 1e-6, "{l}");
        assert!((r - 0.75).abs() < 1e-5, "{r}");
        let (l, r) = channel_gains(position, right, [4.0, 0.0, 0.0], 16.0);
        assert!(r < 1e-6 && (l - 0.75).abs() < 1e-5);
        let (l, r) = channel_gains(position, right, [0.0, 0.0, 8.0], 16.0);
        assert!((l - 0.5).abs() < 1e-5 && (r - 0.5).abs() < 1e-5);
        let (l, r) = channel_gains(position, right, position, 16.0);
        assert!((l - 1.0).abs() < 1e-5 && (r - 1.0).abs() < 1e-5);
    }

    #[test]
    fn turning_swaps_the_sides() {
        let listener = Listener::default();
        listener.set([0.0; 3], 90.0);
        let (position, right) = listener.get();
        let (l, r) = channel_gains(position, right, [0.0, 0.0, -2.0], 16.0);
        assert!(l < 1e-5 && r > 0.8, "{l} {r}");
    }

    #[test]
    fn the_source_scales_each_channel() {
        let listener = Arc::new(Listener::default());
        listener.set([0.0; 3], 0.0);
        let stereo = SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(44_100).unwrap(),
            vec![1.0; 8],
        );
        let heard: Vec<f32> = Spatial::new(stereo, listener, [-4.0, 0.0, 0.0], 16.0).collect();
        assert_eq!(heard.len(), 8);
        assert!(heard[0].abs() < 1e-6 && (heard[1] - 0.75).abs() < 1e-5);
        assert!(heard[6].abs() < 1e-6 && (heard[7] - 0.75).abs() < 1e-5);
    }
}
