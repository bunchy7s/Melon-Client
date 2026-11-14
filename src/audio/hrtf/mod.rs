//! "Directional Audio" (`Options.directionalAudio`): vanilla turns on
//! OpenAL Soft's HRTF, which filters each sound through the head-related
//! impulse responses of where it is, for both ears. This is that, with the
//! same data set and OpenAL Soft's way of picking a response
//! (`GetHrtfCoeffs`), done per sound as its default `hrtf-mode = full` does.

use super::spatial::{Listener, ListenerPose, linear_gain};
use rodio::source::{SeekError, UniformSourceIterator};
use rodio::{ChannelCount, Sample, SampleRate, Source};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// OpenAL Soft's `Default HRTF.mhr`, see `README.md` beside it.
static DEFAULT_MHR: &[u8] = include_bytes!("default.mhr");

/// Taps of a response; the data set has exactly this many.
pub const TAPS: usize = 64;
/// `HRIR_DELAY_FRACBITS`: delays are stored in quarter samples.
const DELAY_FRACTION: f32 = 4.0;
/// Room for the taps and the longest delay (63 quarter samples, 16 whole).
const HISTORY: usize = 128;
/// Frames between two looks at where the listener is: about 5 ms.
const REFRESH_FRAMES: u32 = 256;
/// The rate the responses are for; sounds are brought to it first.
pub const RATE: u32 = 48_000;
/// Where OpenAL Soft puts the two channels of a stereo file (`AL_STEREO_ANGLES`).
const STEREO_ANGLE: f32 = std::f32::consts::FRAC_PI_6;

/// One elevation's ring of measured directions.
#[derive(Clone, Copy, Debug)]
struct Elevation {
    azimuths: usize,
    first: usize,
}

/// A parsed data set: the left ear's responses (the right ear hears the
/// mirrored direction), from straight down to straight up.
#[derive(Debug)]
pub struct HrtfStore {
    rate: u32,
    elevations: Vec<Elevation>,
    /// `[response][tap]`.
    coeffs: Vec<[f32; TAPS]>,
    /// Each response's delay, in samples.
    delays: Vec<f32>,
}

/// The two ears' filters for one direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EarFilters {
    /// `[ear][tap]`, left then right, taps reversed for the convolution.
    taps: [[f32; TAPS]; 2],
    /// Whole samples each ear hears it late.
    delays: [usize; 2],
}

impl HrtfStore {
    /// `LoadHrtf03`: a `MinPHR03` file with one field, mono (left ear) data.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let slice = bytes.get(at..at + n)?;
            at += n;
            Some(slice)
        };
        if take(8)? != b"MinPHR03" {
            return None;
        }
        let rate = u32::from_le_bytes(take(4)?.try_into().ok()?);
        let channel_type = take(1)?[0];
        let taps = usize::from(take(1)?[0]);
        let fields = take(1)?[0];
        if channel_type != 0 || taps != TAPS || fields != 1 {
            return None;
        }
        let _distance = take(2)?;
        let elevation_count = usize::from(take(1)?[0]);
        let mut elevations = Vec::with_capacity(elevation_count);
        let mut first = 0;
        for &azimuths in take(elevation_count)? {
            let azimuths = usize::from(azimuths);
            elevations.push(Elevation { azimuths, first });
            first += azimuths;
        }
        let count = first;
        let mut coeffs = Vec::with_capacity(count);
        for _ in 0..count {
            let raw = take(TAPS * 3)?;
            let mut response = [0.0; TAPS];
            for (tap, value) in response.iter_mut().zip(raw.as_chunks::<3>().0) {
                let int = i32::from_le_bytes([0, value[0], value[1], value[2]]) >> 8;
                *tap = int as f32 / 8_388_608.0;
            }
            coeffs.push(response);
        }
        let delays = take(count)?
            .iter()
            .map(|&d| f32::from(d) / DELAY_FRACTION)
            .collect();
        (at == bytes.len()).then_some(Self {
            rate,
            elevations,
            coeffs,
            delays,
        })
    }

    /// The data set vanilla hears, parsed once.
    pub fn default_set() -> Option<&'static Self> {
        static STORE: OnceLock<Option<HrtfStore>> = OnceLock::new();
        STORE.get_or_init(|| Self::parse(DEFAULT_MHR)).as_ref()
    }

    pub fn len(&self) -> usize {
        self.coeffs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.coeffs.is_empty()
    }

    /// `GetHrtfCoeffs`: the filters for a sound at `elevation` (up is
    /// positive) and `azimuth` (right is positive), radians, blended from
    /// the four nearest measured directions.
    pub fn filters(&self, elevation: f32, azimuth: f32) -> EarFilters {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let last = self.elevations.len() - 1;
        let ev = ((FRAC_PI_2 + elevation) / PI * last as f32).clamp(0.0, last as f32);
        let ev0 = ev.floor() as usize;
        let ev1 = (ev0 + 1).min(last);
        let ev_frac = ev - ev0 as f32;
        // Each ring's two neighbours and their weights, and the mirrored
        // index of each for the right ear.
        let mut picks = [(0usize, 0usize, 0.0f32); 4];
        for (i, (index, weight)) in [(ev0, 1.0 - ev_frac), (ev1, ev_frac)]
            .into_iter()
            .enumerate()
        {
            let ring = self.elevations[index];
            let az = (TAU + azimuth).rem_euclid(TAU) / TAU * ring.azimuths as f32;
            let az0 = az.floor() as usize % ring.azimuths;
            let az1 = (az0 + 1) % ring.azimuths;
            let az_frac = az - az.floor();
            let mirror = |a: usize| ring.first + (ring.azimuths - a) % ring.azimuths;
            picks[i * 2] = (ring.first + az0, mirror(az0), weight * (1.0 - az_frac));
            picks[i * 2 + 1] = (ring.first + az1, mirror(az1), weight * az_frac);
        }
        let mut taps = [[0.0; TAPS]; 2];
        let mut delays = [0.0f32; 2];
        for (left, right, weight) in picks {
            if weight <= 0.0 {
                continue;
            }
            for (ear, index) in [left, right].into_iter().enumerate() {
                delays[ear] += self.delays[index] * weight;
                for (tap, c) in self.coeffs[index].iter().enumerate() {
                    // Reversed, so the convolution reads the history forward.
                    taps[ear][TAPS - 1 - tap] += c * weight;
                }
            }
        }
        EarFilters {
            taps,
            delays: delays.map(|d| d.round() as usize),
        }
    }
}

/// A sound's direction in the listener's frame: elevation and azimuth.
fn direction(pose: &ListenerPose, emitter: [f32; 3]) -> (f32, f32, f32) {
    let delta = [0, 1, 2].map(|i| emitter[i] - pose.position[i]);
    let distance = delta.iter().map(|d| d * d).sum::<f32>().sqrt();
    if distance < 1.0e-4 {
        return (distance, 0.0, 0.0);
    }
    let dot = |a: [f32; 3]| (0..3).map(|i| a[i] * delta[i]).sum::<f32>() / distance;
    let (x, y, z) = (dot(pose.right()), dot(pose.up), dot(pose.forward));
    (distance, y.clamp(-1.0, 1.0).asin(), x.atan2(z))
}

/// One source the head hears: its recent samples and its ears' filters,
/// fading from the old filters to new ones over a refresh.
struct Ears {
    /// The history twice over, so a window never wraps.
    history: Box<[f32; 2 * HISTORY]>,
    pos: usize,
    old: EarFilters,
    new: EarFilters,
    old_gain: f32,
    new_gain: f32,
}

impl Ears {
    fn new(filters: EarFilters, gain: f32) -> Self {
        Self {
            history: Box::new([0.0; 2 * HISTORY]),
            pos: 0,
            old: filters,
            new: filters,
            old_gain: gain,
            new_gain: gain,
        }
    }

    fn retarget(&mut self, filters: EarFilters, gain: f32) {
        self.old = self.new;
        self.old_gain = self.new_gain;
        self.new = filters;
        self.new_gain = gain;
    }

    fn push(&mut self, sample: f32) {
        self.pos = (self.pos + 1) % HISTORY;
        self.history[self.pos] = sample;
        self.history[self.pos + HISTORY] = sample;
    }

    fn convolve(&self, filters: &EarFilters, ear: usize) -> f32 {
        let newest = self.pos + HISTORY - filters.delays[ear];
        let window = &self.history[newest + 1 - TAPS..=newest];
        window
            .iter()
            .zip(&filters.taps[ear])
            .map(|(x, c)| x * c)
            .sum()
    }

    /// Both ears now, `fade` of the way (0 to 1) from the old filters.
    fn hear(&self, fade: f32) -> [f32; 2] {
        let steady = self.old == self.new;
        [0, 1].map(|ear| {
            let new = self.convolve(&self.new, ear);
            let gain = self.old_gain + (self.new_gain - self.old_gain) * fade;
            if steady {
                return new * gain;
            }
            let old = self.convolve(&self.old, ear);
            (old * (1.0 - fade) * self.old_gain) + (new * fade * self.new_gain)
        })
    }
}

/// A sound with a place, heard through the HRTF: two channels at
/// [`RATE`], for headphones.
pub struct Binaural<I: Source> {
    input: UniformSourceIterator<I>,
    /// 1 for a mono file, 2 for a stereo one (each side its own source).
    inputs: usize,
    store: &'static HrtfStore,
    listener: Arc<Listener>,
    emitter: [f32; 3],
    range: f32,
    sources: Vec<Ears>,
    frame: [f32; 2],
    /// The output channel the next sample is for.
    channel: usize,
    /// Frames into the current refresh.
    step: u32,
}

impl<I: Source> Binaural<I> {
    pub fn new(
        input: I,
        store: &'static HrtfStore,
        listener: Arc<Listener>,
        emitter: [f64; 3],
        range: f32,
    ) -> Self {
        let inputs = usize::from(input.channels().get()).clamp(1, 2);
        let channels = ChannelCount::new(inputs as u16).expect("one or two");
        let rate = SampleRate::new(store.rate).expect("a data set has a rate");
        let mut binaural = Self {
            input: UniformSourceIterator::new(input, channels, rate),
            inputs,
            store,
            listener,
            emitter: emitter.map(|c| c as f32),
            range,
            sources: Vec::with_capacity(inputs),
            frame: [0.0; 2],
            channel: 0,
            step: 0,
        };
        for (filters, gain) in binaural.targets().into_iter().take(inputs) {
            binaural.sources.push(Ears::new(filters, gain));
        }
        binaural
    }

    /// Each source's filters and gain for where the listener is now. A
    /// stereo file's sides stay at ±30° from the listener and only fade
    /// with distance, as OpenAL Soft renders multi-channel sources.
    /// Without allocating: it runs on the audio thread. Only the first
    /// [`Self::inputs`] entries are meant.
    fn targets(&self) -> [(EarFilters, f32); 2] {
        let pose = self.listener.get();
        let (distance, elevation, azimuth) = direction(&pose, self.emitter);
        let gain = linear_gain(distance, self.range);
        if self.inputs == 1 {
            let filters = self.store.filters(elevation, azimuth);
            [(filters, gain), (filters, gain)]
        } else {
            [-STEREO_ANGLE, STEREO_ANGLE].map(|side| (self.store.filters(0.0, side), gain))
        }
    }
}

impl<I: Source> Iterator for Binaural<I> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.channel == 0 {
            if self.step == REFRESH_FRAMES {
                self.step = 0;
                let targets = self.targets();
                for (ears, (filters, gain)) in self.sources.iter_mut().zip(targets) {
                    ears.retarget(filters, gain);
                }
            }
            let fade = self.step as f32 / REFRESH_FRAMES as f32;
            self.step += 1;
            let mut out = [0.0; 2];
            for ears in &mut self.sources {
                ears.push(self.input.next()?);
                let heard = ears.hear(fade);
                out[0] += heard[0];
                out[1] += heard[1];
            }
            self.frame = out;
        }
        let sample = self.frame[self.channel];
        self.channel ^= 1;
        Some(sample)
    }
}

impl<I: Source> Source for Binaural<I> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).expect("two")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.store.rate).expect("a data set has a rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.channel = 0;
        self.input.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::f32::consts::FRAC_PI_2;

    fn store() -> &'static HrtfStore {
        HrtfStore::default_set().expect("the vendored data set parses")
    }

    #[test]
    fn the_data_set_is_read_whole() {
        let store = store();
        assert_eq!(store.rate, RATE);
        assert_eq!(store.len(), 828);
        assert_eq!(store.elevations.len(), 19);
        assert_eq!(store.elevations[0].azimuths, 1);
        assert_eq!(store.elevations[9].azimuths, 72);
        assert!(HrtfStore::parse(&DEFAULT_MHR[..100]).is_none());
    }

    fn energy(taps: &[f32; TAPS]) -> f32 {
        taps.iter().map(|t| t * t).sum()
    }

    #[test]
    fn a_sound_on_the_right_is_louder_and_sooner_in_the_right_ear() {
        let right = store().filters(0.0, FRAC_PI_2);
        assert!(energy(&right.taps[1]) > 2.0 * energy(&right.taps[0]));
        assert!(right.delays[0] > right.delays[1], "{:?}", right.delays);
        let left = store().filters(0.0, -FRAC_PI_2);
        assert!(energy(&left.taps[0]) > 2.0 * energy(&left.taps[1]));
        // Straight ahead both ears hear the same.
        let ahead = store().filters(0.0, 0.0);
        assert_eq!(ahead.delays[0], ahead.delays[1]);
        let (l, r) = (energy(&ahead.taps[0]), energy(&ahead.taps[1]));
        assert!((l - r).abs() < 1e-6 * l.max(1e-9), "{l} {r}");
    }

    #[test]
    fn a_moving_sound_stays_finite_and_complete() {
        let listener = Arc::new(Listener::default());
        listener.set([0.0; 3], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
        let one = ChannelCount::new(1).unwrap();
        let rate = SampleRate::new(RATE).unwrap();
        let noise: Vec<f32> = (0..4000)
            .map(|i| ((i * 7919) % 200) as f32 / 100.0 - 1.0)
            .collect();
        let binaural = Binaural::new(
            SamplesBuffer::new(one, rate, noise),
            store(),
            Arc::clone(&listener),
            [4.0, 0.0, 0.0],
            16.0,
        );
        let mut heard = Vec::new();
        for (i, sample) in binaural.enumerate() {
            if i == 2000 {
                listener.set([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
            }
            heard.push(sample);
        }
        assert_eq!(heard.len(), 8000);
        assert!(heard.iter().all(|s| s.is_finite()));
        // East of a listener facing north is the right ear.
        let ear = |e: usize| {
            heard[..2000]
                .iter()
                .skip(e)
                .step_by(2)
                .map(|s| s * s)
                .sum::<f32>()
        };
        assert!(ear(1) > ear(0));
    }
}
