//! Sound, through `rodio` (cpal output + symphonia vorbis). cpal's ALSA

pub mod block_sounds;
mod device;
pub mod level_sounds;
pub mod library;
pub mod music;
pub mod source;
pub mod spatial;
pub mod stats;

use crate::assets::Assets;
use library::{Sound, SoundEvents};
use rodio::buffer::SamplesBuffer;
use rodio::mixer::Mixer;
use rodio::source::UniformSourceIterator;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Sample, Source};
use source::{SoundSource, Volumes};
use spatial::{Listener, Spatial};
use stats::{AudioStats, Counted};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The button click (`SoundEvents.UI_BUTTON_CLICK`).
const CLICK: &str = "ui.button.click";
/// How often a playing sound looks at its source's volume.
const VOLUME_POLL: Duration = Duration::from_millis(10);
/// What a sound with a place is turned into before it is panned.
const STEREO: rodio::ChannelCount = std::num::NonZeroU16::new(2).unwrap();
/// `SoundEngine.calculatePitch`.
const PITCH_RANGE: (f32, f32) = (0.5, 2.0);

/// A sound to play: `SimpleSoundInstance`'s event, source, volume, pitch
pub struct SoundInstance<'a> {
    /// The sound event, `block.stone.break`.
    pub event: Cow<'a, str>,
    pub source: SoundSource,
    /// Above 1 the sound carries further (`Sound.getAttenuationDistance`).
    pub volume: f32,
    pub pitch: f32,
    /// Where it is heard from, in the world. A sound without a place is
    pub position: Option<[f64; 3]>,
}

impl<'a> SoundInstance<'a> {
    /// The event at full volume and normal pitch, on its default source.
    pub fn of(event: impl Into<Cow<'a, str>>) -> Self {
        let event = event.into();
        Self {
            source: SoundSource::for_event(&event),
            event,
            volume: 1.0,
            pitch: 1.0,
            position: None,
        }
    }

    pub fn on(mut self, source: SoundSource) -> Self {
        self.source = source;
        self
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }

    /// Heard from this place in the world.
    pub fn at(mut self, position: [f64; 3]) -> Self {
        self.position = Some(position);
        self
    }
}

/// What each source is worth right now (volume times master), shared with
#[derive(Default)]
struct Gains([AtomicU32; 11]);

impl Gains {
    fn get(&self, source: SoundSource) -> f32 {
        f32::from_bits(self.0[source.index()].load(Ordering::Relaxed))
    }

    fn set(&self, volumes: &Volumes) {
        for source in SoundSource::ALL {
            let gain = volumes.final_volume(source);
            self.0[source.index()].store(gain.to_bits(), Ordering::Relaxed);
        }
    }
}

/// `MELON_SOUND_SOURCE=<name>` plays every sound on that source, to try
const FORCED_SOURCE_VAR: &str = "MELON_SOUND_SOURCE";

pub struct AudioEngine {
    _stream: Option<MixerDeviceSink>,
    mixer: Option<Mixer>,
    forced_source: Option<SoundSource>,
    listener: Arc<Listener>,
    events: SoundEvents,
    sounds_root: PathBuf,
    decoded: Arc<Mutex<HashMap<Arc<str>, Option<SamplesBuffer>>>>,
    waiting: Arc<Mutex<HashMap<Arc<str>, Vec<Voice>>>>,
    gains: Arc<Gains>,
    stats: Arc<AudioStats>,
}

impl AudioEngine {
    /// Missing device or sound file: playing is a no-op. `device` is the
    pub fn init(assets: &Assets, device: &str) -> Self {
        let stream = open_output(device);
        if stream.is_none() {
            eprintln!("Audio-Ausgabe nicht verfuegbar, Sounds bleiben stumm");
        }
        let mixer = stream.as_ref().map(|s| s.mixer().clone());
        let forced_source = std::env::var(FORCED_SOURCE_VAR)
            .ok()
            .and_then(|name| SoundSource::from_name(&name));
        let gains = Arc::new(Gains::default());
        gains.set(&Volumes::default());
        Self {
            _stream: stream,
            mixer,
            forced_source,
            listener: Arc::new(Listener::default()),
            events: SoundEvents::load(assets.root()),
            sounds_root: assets.root().to_path_buf(),
            decoded: Arc::default(),
            waiting: Arc::default(),
            gains,
            stats: Arc::default(),
        }
    }

    /// The output devices to choose from, by name.
    pub fn reload_sounds(&mut self, assets: &Assets) {
        self.events = SoundEvents::load(assets.root());
        self.decoded = Arc::default();
        self.waiting = Arc::default();
    }

    pub fn devices(&self) -> Vec<String> {
        device::output_device_names()
    }

    /// Moves the output to the device with this name (the system default
    pub fn set_device(&mut self, device: &str) {
        self._stream = open_output(device);
        self.mixer = self._stream.as_ref().map(|s| s.mixer().clone());
    }

    /// Where the player's ears are, for sounds that have a place: call it
    pub fn set_listener(&self, position: [f64; 3], yaw: f32) {
        self.listener.set(position, yaw);
    }

    /// The sliders changed: every sound, playing or not, follows.
    pub fn set_volumes(&self, volumes: &Volumes) {
        self.gains.set(volumes);
    }

    /// What the engine did so far, for F3 and the benchmark.
    pub fn stats(&self) -> &AudioStats {
        &self.stats
    }

    /// Vanilla `AbstractButton.playDownSound` (`SoundEvents.UI_BUTTON_CLICK`).
    pub fn play_click(&self) {
        self.play(&SoundInstance::of(CLICK));
    }

    /// Starts a sound. The mixer gets a pre-decoded `SamplesBuffer`; a live
    pub fn play(&self, instance: &SoundInstance) {
        let started = std::time::Instant::now();
        if self.start(instance) {
            self.stats.played(started.elapsed().as_nanos() as u64);
        }
    }

    /// [`Self::play`]; whether the sound started or is being loaded.
    fn start(&self, instance: &SoundInstance) -> bool {
        let Some(mixer) = &self.mixer else {
            return false;
        };
        let Some(sound) = self.events.pick(&instance.event, rand::random::<f32>()) else {
            return false;
        };
        let volume = instance.volume.clamp(0.0, 1.0) * sound.volume;
        let voice = Voice {
            mixer: mixer.clone(),
            listener: Arc::clone(&self.listener),
            gains: Arc::clone(&self.gains),
            stats: Arc::clone(&self.stats),
            volume,
            pitch: (instance.pitch * sound.pitch).clamp(PITCH_RANGE.0, PITCH_RANGE.1),
            source: self.forced_source.unwrap_or(instance.source),
            place: instance
                .position
                .map(|at| (at, instance.volume.max(1.0) * sound.attenuation_distance)),
        };
        let cached = if sound.stream {
            None
        } else {
            self.decoded
                .lock()
                .ok()
                .and_then(|decoded| decoded.get(&*sound.name).cloned())
        };
        match cached {
            Some(Some(buffer)) => voice.add(Box::new(buffer)),
            Some(None) => return false,
            None => {
                let name = Arc::clone(&sound.name);
                if !sound.stream {
                    let Ok(mut waiting) = self.waiting.lock() else {
                        return false;
                    };
                    if let Some(voices) = waiting.get_mut(&name) {
                        voices.push(voice);
                        return true;
                    }
                    waiting.insert(Arc::clone(&name), Vec::new());
                }
                let root = self.sounds_root.clone();
                let decoded = Arc::clone(&self.decoded);
                let waiting = Arc::clone(&self.waiting);
                let task_name = Arc::clone(&name);
                let spawned = std::thread::Builder::new()
                    .name("sound-load".to_owned())
                    .spawn(move || {
                        if sound.stream {
                            if let Some(input) = open_stream(&root, &sound) {
                                voice.add(Box::new(input));
                            }
                            return;
                        }
                        let buffer = load_buffer(&root, &sound, &decoded, &voice.stats);
                        let others = waiting
                            .lock()
                            .ok()
                            .and_then(|mut w| w.remove(&task_name))
                            .unwrap_or_default();
                        if let Some(buffer) = buffer {
                            for voice in std::iter::once(voice).chain(others) {
                                voice.add(Box::new(buffer.clone()));
                            }
                        }
                    });
                if let Err(e) = spawned {
                    if let Ok(mut waiting) = self.waiting.lock() {
                        waiting.remove(&name);
                    }
                    eprintln!("Audio: kein Lade-Thread fuer {}: {e}", instance.event);
                    return false;
                }
            }
        }
        true
    }
}

/// A sound about to start: where it goes and how loud, everything it needs
struct Voice {
    mixer: Mixer,
    listener: Arc<Listener>,
    gains: Arc<Gains>,
    stats: Arc<AudioStats>,
    volume: f32,
    pitch: f32,
    source: SoundSource,
    /// Where it is heard from, and how far it carries.
    place: Option<([f64; 3], f32)>,
}

impl Voice {
    fn add(self, input: Box<dyn Source<Item = Sample> + Send>) {
        let input = input.speed(self.pitch);
        let input: Box<dyn Source<Item = Sample> + Send> = match self.place {
            Some((at, range)) => {
                let rate = input.sample_rate();
                let stereo = UniformSourceIterator::new(input, STEREO, rate);
                Box::new(Spatial::new(stereo, self.listener, at, range))
            }
            None => Box::new(input),
        };
        let (volume, source, gains) = (self.volume, self.source, self.gains);
        let input =
            input
                .amplify(volume * gains.get(source))
                .periodic_access(VOLUME_POLL, move |s| {
                    s.set_factor(volume * gains.get(source));
                });
        self.mixer.add(Counted::new(input, self.stats));
    }
}

/// The decoded file of `sound`, decoded and remembered the first time. A
fn load_buffer(
    root: &Path,
    sound: &Sound,
    decoded: &Mutex<HashMap<Arc<str>, Option<SamplesBuffer>>>,
    stats: &AudioStats,
) -> Option<SamplesBuffer> {
    if let Some(known) = decoded.lock().ok()?.get(&*sound.name) {
        return known.clone();
    }
    let started = std::time::Instant::now();
    let buffer = decode(root, &sound.name);
    stats.decoded(started.elapsed().as_nanos() as u64);
    let mut decoded = decoded.lock().ok()?;
    let entry = decoded.entry(sound.name.clone()).or_insert_with(|| {
        if buffer.is_none() {
            eprintln!("Audio: {}.ogg fehlt oder ist unlesbar", sound.name);
        }
        buffer
    });
    entry.clone()
}

/// The named device, else the best the system has.
fn open_output(name: &str) -> Option<MixerDeviceSink> {
    let picked = device::find_output_device(name).or_else(device::pick_output_device);
    let opened = picked
        .and_then(|d| DeviceSinkBuilder::from_device(d).ok())
        .and_then(|b| b.open_stream().ok());
    opened.or_else(|| DeviceSinkBuilder::open_default_sink().ok())
}

fn decode(assets_root: &Path, name: &str) -> Option<SamplesBuffer> {
    let bytes = crate::assets::sound(assets_root, name)?;
    let decoder = Decoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (channels, sample_rate) = (decoder.channels(), decoder.sample_rate());
    Some(SamplesBuffer::new(
        channels,
        sample_rate,
        decoder.collect::<Vec<f32>>(),
    ))
}

/// Music: decoded while it plays, from the file read whole (it may sit in
fn open_stream(assets_root: &Path, sound: &Sound) -> Option<Decoder<std::io::Cursor<Vec<u8>>>> {
    let bytes = crate::assets::sound(assets_root, &sound.name)?;
    match Decoder::new(std::io::Cursor::new(bytes)) {
        Ok(decoder) => Some(decoder),
        Err(e) => {
            eprintln!("Audio: {}.ogg nicht abspielbar: {e}", sound.name);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode-only: the `.ogg` yields stereo samples without an OS audio device.
    #[test]
    fn click_sound_decodes() {
        let bytes = crate::assets::sound(&crate::assets::default_root(), "random/click_stereo")
            .expect("assets/minecraft/sounds/random/click_stereo.ogg nicht lesbar");
        let mut decoder = Decoder::new(std::io::Cursor::new(bytes))
            .expect("Decoder::new sollte click_stereo.ogg dekodieren");
        assert_eq!(
            decoder.channels().get(),
            2,
            "click_stereo.ogg sollte stereo sein"
        );
        let sample_count = (0..).take_while(|_| decoder.next().is_some()).count();
        assert!(
            sample_count > 1000,
            "erwarte mehrere tausend Samples, bekam {sample_count}"
        );
    }

    #[test]
    fn gains_follow_the_volumes() {
        let gains = Gains::default();
        let mut volumes = Volumes::default();
        volumes.set(SoundSource::Master, 0.5);
        volumes.set(SoundSource::Music, 0.5);
        gains.set(&volumes);
        assert_eq!(gains.get(SoundSource::Ui), 0.5);
        assert_eq!(gains.get(SoundSource::Music), 0.25);
    }

    #[test]
    fn an_instance_defaults_to_its_events_source() {
        let sound = SoundInstance::of("block.stone.break");
        assert_eq!(sound.source, SoundSource::Blocks);
        assert_eq!(sound.on(SoundSource::Music).source, SoundSource::Music);
    }
}
