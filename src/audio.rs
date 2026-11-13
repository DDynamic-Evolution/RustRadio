use std::{
    fs::File,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source, mixer, source::Zero};

use crate::broadcast::{self, BLOCK_SAMPLES, CHANNELS, Pcm, RATE};

use crate::{
    equalizer::{BANDS, Equalizer},
    microphone,
};

pub const DUCK_GAIN: f32 = 0.25;
type Feed = Arc<Mutex<Option<SyncSender<Pcm>>>>;

pub struct Audio {
    mixer: mixer::Mixer,
    feed: Feed,
    recording_feed: Feed,
    stream: OutputStream,
    render: Arc<Mutex<(u64, Tap)>>,
    mic: Arc<Mutex<Option<microphone::Input>>>,
    gains: Arc<Mutex<[f32; BANDS.len()]>>,
    stream_gain: Arc<AtomicU32>,
    local_gain: Arc<AtomicU32>,
    stream_peak: Arc<AtomicU32>,
    local_peak: Arc<AtomicU32>,
    mic_gain: Arc<AtomicU32>,
    mic_peak: Arc<AtomicU32>,
}

impl Audio {
    pub fn open(selection: Option<&crate::devices::Selection>) -> Result<Self, String> {
        let stream = open_output(selection)?;
        let (mixer, tap) = program_mix();
        let feed = tap.feed.clone();
        let recording_feed = tap.recording_feed.clone();
        let mic = tap.mic.clone();
        let gains = tap.gains.clone();
        let stream_gain = tap.stream_gain.clone();
        let local_gain = tap.local_gain.clone();
        let stream_peak = tap.stream_peak.clone();
        let local_peak = tap.local_peak.clone();
        let mic_gain = tap.mic_gain.clone();
        let mic_peak = tap.mic_peak.clone();
        let render = Arc::new(Mutex::new((0, tap)));
        stream.mixer().add(Output {
            render: render.clone(),
            generation: 0,
        });
        Ok(Self {
            mixer,
            feed,
            recording_feed,
            stream,
            render,
            mic,
            gains,
            stream_gain,
            local_gain,
            stream_peak,
            local_peak,
            mic_gain,
            mic_peak,
        })
    }

    pub fn change_output(
        &mut self,
        selection: Option<&crate::devices::Selection>,
    ) -> Result<(), String> {
        let stream = open_output(selection)?;
        let generation = {
            let mut render = self.render.lock().unwrap();
            render.0 += 1;
            render.0
        };
        stream.mixer().add(Output {
            render: self.render.clone(),
            generation,
        });
        self.stream = stream;
        Ok(())
    }

    pub fn set_microphone(&self, mic: Option<microphone::Input>) {
        *self.mic.lock().unwrap() = mic;
        self.mic_peak.store(0, Ordering::Relaxed);
    }

    pub fn set_eq(&self, gains: [f32; BANDS.len()]) {
        *self.gains.lock().unwrap() = gains;
    }

    pub fn set_mic_gain(&self, gain: f32) {
        self.mic_gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn mic_peak(&self) -> f32 {
        f32::from_bits(self.mic_peak.load(Ordering::Relaxed))
    }

    pub fn set_stream_gain(&self, gain: f32) {
        self.stream_gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn stream_peak(&self) -> f32 {
        f32::from_bits(self.stream_peak.load(Ordering::Relaxed))
    }

    pub fn set_local_gain(&self, gain: f32) {
        self.local_gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn local_peak(&self) -> f32 {
        f32::from_bits(self.local_peak.load(Ordering::Relaxed))
    }

    pub fn set_feed(&self, sender: Option<SyncSender<Pcm>>) {
        *self.feed.lock().unwrap() = sender;
    }

    pub fn set_recording_feed(&self, sender: Option<SyncSender<Pcm>>) {
        *self.recording_feed.lock().unwrap() = sender;
    }

    pub fn deck(&self, path: &Path, gain: f32) -> Result<Deck, String> {
        let source = File::open(path)
            .map_err(|error| error.to_string())
            .and_then(|file| Decoder::try_from(file).map_err(|error| error.to_string()))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let duration = source.total_duration();
        let player = Sink::connect_new(&self.mixer);
        player.set_volume(gain);
        player.append(source);
        Ok(Deck { player, duration })
    }
}

fn open_output(selection: Option<&crate::devices::Selection>) -> Result<OutputStream, String> {
    let device = crate::devices::Direction::Output.resolve(selection)?;
    OutputStreamBuilder::from_device(device)
        .and_then(OutputStreamBuilder::open_stream)
        .map_err(|error| format!("Audio output: {error}"))
}

struct Output {
    render: Arc<Mutex<(u64, Tap)>>,
    generation: u64,
}

impl Iterator for Output {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let mut render = self.render.lock().unwrap();
        if render.0 == self.generation {
            render.1.next()
        } else {
            None
        }
    }
}

impl Source for Output {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct Deck {
    pub player: Sink,
    duration: Option<Duration>,
}

impl Deck {
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.duration
            .map(|duration| duration.saturating_sub(self.player.get_pos()))
    }
}

fn program_mix() -> (mixer::Mixer, Tap) {
    let (mixer, source) = mixer::mixer(CHANNELS, RATE);
    mixer.add(Zero::new(CHANNELS, RATE));
    let tap = Tap {
        source,
        feed: Feed::default(),
        recording_feed: Feed::default(),
        block: [0; BLOCK_SAMPLES * 4],
        offset: 0,
        mic: Arc::default(),
        gains: Arc::default(),
        eq: Equalizer::default(),
        voice: 0.0,
        music_gain: 1.0,
        stream_gain: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
        local_gain: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
        stream_peak: Arc::default(),
        local_peak: Arc::default(),
        mic_gain: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
        mic_peak: Arc::default(),
        mic_block_peak: 0.0,
        mic_block_gain: 1.0,
        block_peak: 0.0,
        block_gain: 1.0,
        local_block_gain: 1.0,
        local_block_peak: 0.0,
    };
    (mixer, tap)
}

struct Tap {
    source: mixer::MixerSource,
    feed: Feed,
    recording_feed: Feed,
    block: Pcm,
    offset: usize,
    mic: Arc<Mutex<Option<microphone::Input>>>,
    gains: Arc<Mutex<[f32; BANDS.len()]>>,
    stream_gain: Arc<AtomicU32>,
    local_gain: Arc<AtomicU32>,
    stream_peak: Arc<AtomicU32>,
    local_peak: Arc<AtomicU32>,
    mic_gain: Arc<AtomicU32>,
    mic_peak: Arc<AtomicU32>,
    eq: Equalizer,
    voice: f32,
    music_gain: f32,
    block_peak: f32,
    mic_block_peak: f32,
    mic_block_gain: f32,
    block_gain: f32,
    local_block_gain: f32,
    local_block_peak: f32,
}

impl Iterator for Tap {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.offset == 0 {
            self.mic_block_gain = f32::from_bits(self.mic_gain.load(Ordering::Relaxed));
            self.local_block_gain = f32::from_bits(self.local_gain.load(Ordering::Relaxed));
            self.block_gain = f32::from_bits(self.stream_gain.load(Ordering::Relaxed));
            if let Ok(gains) = self.gains.try_lock() {
                self.eq.configure(*gains);
            }
        }
        let channel = self.offset / 4 % CHANNELS as usize;
        if channel == 0
            && let Ok(mut mic) = self.mic.try_lock()
        {
            let target = if mic.is_some() { DUCK_GAIN } else { 1.0 };
            self.music_gain += (target - self.music_gain) * 0.002;
            self.voice = mic.as_mut().map_or(0.0, |input| input.read(RATE)) * self.mic_block_gain;
            self.mic_block_peak = self.mic_block_peak.max(self.voice.abs());
        }
        let music = self.eq.process(self.source.next()?, channel) * self.music_gain;
        let local = (music * self.local_block_gain).clamp(-1.0, 1.0);
        self.local_block_peak = self.local_block_peak.max(local.abs());
        let program = ((music + self.voice) * self.block_gain).clamp(-1.0, 1.0);
        self.block_peak = self.block_peak.max(program.abs());
        self.block[self.offset..self.offset + 4].copy_from_slice(&program.to_le_bytes());
        self.offset += 4;
        if self.offset == self.block.len() {
            publish_peak(&self.stream_peak, &mut self.block_peak);
            publish_peak(&self.local_peak, &mut self.local_block_peak);
            publish_peak(&self.mic_peak, &mut self.mic_block_peak);
            if let Ok(mut feed) = self.feed.try_lock() {
                broadcast::send_pcm(&mut feed, self.block);
            }
            if let Ok(mut feed) = self.recording_feed.try_lock() {
                broadcast::send_pcm(&mut feed, self.block);
            }
            self.offset = 0;
        }
        Some(local)
    }
}

fn publish_peak(meter: &AtomicU32, block_peak: &mut f32) {
    // 1.2 dB of decay per 20 ms block keeps short peaks visible.
    let previous = f32::from_bits(meter.load(Ordering::Relaxed));
    meter.store(
        block_peak.max(previous * 0.871).to_bits(),
        Ordering::Relaxed,
    );
    *block_peak = 0.0;
}

impl Source for Tap {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    #[test]
    fn microphone_ducks_music_and_reaches_only_the_stream() {
        let (mixer, mut tap) = program_mix();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        mixer.add(SamplesBuffer::new(
            CHANNELS,
            RATE,
            vec![0.4; BLOCK_SAMPLES * 16],
        ));
        *tap.mic.lock().unwrap() = Some(microphone::Input::synthetic(
            std::iter::repeat_n(0.3, BLOCK_SAMPLES * 8),
            RATE,
        ));
        tap.mic_gain.store(0.5_f32.to_bits(), Ordering::Relaxed);
        tap.by_ref().nth(BLOCK_SAMPLES * 7 - 1).unwrap();
        *tap.feed.lock().unwrap() = Some(sender);
        let monitor = tap.by_ref().nth(BLOCK_SAMPLES - 1).unwrap();
        let block = receiver.try_recv().unwrap();
        let stream = f32::from_le_bytes(block[..4].try_into().unwrap());
        assert!((monitor - 0.1).abs() < 0.001);
        assert!((stream - 0.25).abs() < 0.001);
        assert_eq!(f32::from_bits(tap.mic_peak.load(Ordering::Relaxed)), 0.15);

        *tap.mic.lock().unwrap() = None;
        let restored = tap.by_ref().nth(BLOCK_SAMPLES * 7 - 1).unwrap();
        assert!((restored - 0.4).abs() < 0.001);
    }

    #[test]
    fn stream_gain_and_meter_follow_encoder_input_without_changing_monitor() {
        let (mixer, mut tap) = program_mix();
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        *tap.feed.lock().unwrap() = Some(sender);
        tap.stream_gain.store(0.5_f32.to_bits(), Ordering::Relaxed);
        tap.local_gain.store(0.25_f32.to_bits(), Ordering::Relaxed);

        for gain in [0.25, 0.5] {
            mixer.add(SamplesBuffer::new(
                CHANNELS,
                RATE,
                vec![gain; BLOCK_SAMPLES],
            ));
        }
        assert!(
            tap.by_ref()
                .take(BLOCK_SAMPLES)
                .all(|sample| sample == 0.1875)
        );
        let block = receiver.try_recv().unwrap();
        assert!(
            block
                .chunks_exact(4)
                .all(|sample| sample == 0.375_f32.to_le_bytes())
        );
        assert_eq!(
            f32::from_bits(tap.stream_peak.load(Ordering::Relaxed)),
            0.375
        );
        assert_eq!(
            f32::from_bits(tap.local_peak.load(Ordering::Relaxed)),
            0.1875
        );
        assert_eq!(tap.next(), Some(0.0));
    }
}
