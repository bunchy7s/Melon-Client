//! What the sound engine did, for F3 and the benchmark: sounds started and

use rodio::{ChannelCount, SampleRate, Source};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Duration;

#[derive(Default)]
pub struct AudioStats {
    started: AtomicU64,
    ended: AtomicU64,
    /// Time in [`super::AudioEngine::play`], decoding included.
    play_nanos: AtomicU64,
    decodes: AtomicU64,
    decode_nanos: AtomicU64,
}

/// The counters at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioTotals {
    pub started: u64,
    pub playing: u64,
    pub play_nanos: u64,
    pub decodes: u64,
    pub decode_nanos: u64,
}

impl AudioStats {
    pub(super) fn played(&self, nanos: u64) {
        self.started.fetch_add(1, Relaxed);
        self.play_nanos.fetch_add(nanos, Relaxed);
    }

    pub(super) fn decoded(&self, nanos: u64) {
        self.decodes.fetch_add(1, Relaxed);
        self.decode_nanos.fetch_add(nanos, Relaxed);
    }

    pub fn totals(&self) -> AudioTotals {
        let ended = self.ended.load(Relaxed);
        let started = self.started.load(Relaxed);
        AudioTotals {
            started,
            playing: started.saturating_sub(ended),
            play_nanos: self.play_nanos.load(Relaxed),
            decodes: self.decodes.load(Relaxed),
            decode_nanos: self.decode_nanos.load(Relaxed),
        }
    }
}

/// A sound that counts itself out when the mixer drops it.
pub(super) struct Counted<S> {
    inner: S,
    stats: Arc<AudioStats>,
}

impl<S> Counted<S> {
    pub(super) fn new(inner: S, stats: Arc<AudioStats>) -> Self {
        Self { inner, stats }
    }
}

impl<S> Drop for Counted<S> {
    fn drop(&mut self) {
        self.stats.ended.fetch_add(1, Relaxed);
    }
}

impl<S: Source> Iterator for Counted<S> {
    type Item = S::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Counted<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[test]
    fn a_dropped_sound_stops_counting() {
        let stats = Arc::new(AudioStats::default());
        stats.played(10);
        let one = std::num::NonZeroU16::new(1).unwrap();
        let rate = std::num::NonZeroU32::new(48_000).unwrap();
        let sound = Counted::new(
            SamplesBuffer::new(one, rate, vec![0.0; 4]),
            Arc::clone(&stats),
        );
        assert_eq!(stats.totals().playing, 1);
        assert_eq!(sound.count(), 4);
        assert_eq!(stats.totals().playing, 0);
        assert_eq!(stats.totals().started, 1);
    }
}
