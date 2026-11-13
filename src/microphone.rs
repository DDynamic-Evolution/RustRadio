use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use cpal::{
    FromSample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};

pub struct Capture {
    _stream: Stream,
    pub input: Input,
    error: Arc<Mutex<Option<String>>>,
}

impl Capture {
    pub fn open(selection: Option<&crate::devices::Selection>) -> Result<Self, String> {
        let device = crate::devices::Direction::Input.resolve(selection)?;
        let supported = device
            .default_input_config()
            .map_err(|error| error.to_string())?;
        let config = supported.config();
        let rate = config.sample_rate.0;
        let queue = Arc::new(Mutex::new(VecDeque::with_capacity((rate / 8) as usize)));
        let error = Arc::new(Mutex::new(None));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => capture::<f32>(&device, &config, &queue, &error),
            SampleFormat::I16 => capture::<i16>(&device, &config, &queue, &error),
            SampleFormat::U16 => capture::<u16>(&device, &config, &queue, &error),
            SampleFormat::I32 => capture::<i32>(&device, &config, &queue, &error),
            SampleFormat::F64 => capture::<f64>(&device, &config, &queue, &error),
            format => return Err(format!("Unsupported microphone sample format: {format}")),
        }
        .map_err(|error| error.to_string())?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            _stream: stream,
            input: Input {
                queue,
                rate,
                phase: 0,
                sample: None,
            },
            error,
        })
    }

    pub fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap().take()
    }
}

fn capture<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    queue: &Arc<Mutex<VecDeque<f32>>>,
    error: &Arc<Mutex<Option<String>>>,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let queue = queue.clone();
    let error = error.clone();
    let channels = usize::from(config.channels);
    let capacity = (config.sample_rate.0 / 8) as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            if let Ok(mut queue) = queue.try_lock() {
                for frame in data.chunks_exact(channels) {
                    let mono = frame
                        .iter()
                        .map(|sample| sample.to_sample::<f32>())
                        .sum::<f32>()
                        / channels as f32;
                    if queue.len() == capacity {
                        queue.pop_front();
                    }
                    queue.push_back(mono);
                }
            }
        },
        move |failure| *error.lock().unwrap() = Some(failure.to_string()),
        None,
    )
}

#[derive(Clone)]
pub struct Input {
    queue: Arc<Mutex<VecDeque<f32>>>,
    rate: u32,
    phase: u64,
    sample: Option<f32>,
}

impl Input {
    #[cfg(test)]
    pub fn synthetic(samples: impl IntoIterator<Item = f32>, rate: u32) -> Self {
        Self {
            queue: Arc::new(Mutex::new(samples.into_iter().collect())),
            rate,
            phase: 0,
            sample: None,
        }
    }

    pub fn read(&mut self, output_rate: u32) -> f32 {
        let Ok(mut queue) = self.queue.try_lock() else {
            return 0.0;
        };
        self.sample = self.sample.or_else(|| queue.pop_front());
        let value = self.sample.unwrap_or(0.0);
        self.phase += u64::from(self.rate);
        while self.phase >= u64::from(output_rate) {
            self.sample = queue.pop_front();
            self.phase -= u64::from(output_rate);
        }
        value
    }
}
