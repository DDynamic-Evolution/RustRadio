pub const BANDS: [f32; 5] = [80.0, 250.0, 1_000.0, 4_000.0, 12_000.0];

#[derive(Clone, Copy, Default)]
struct Filter {
    coefficients: [f32; 5],
    state: [[f32; 2]; 2],
    gain: f32,
}

impl Filter {
    fn configure(&mut self, frequency: f32, gain: f32) {
        if self.gain == gain {
            return;
        }
        self.gain = gain;
        self.state = [[0.0; 2]; 2];
        let amplitude = 10.0_f32.powf(gain / 40.0);
        let omega = std::f32::consts::TAU * frequency / crate::broadcast::RATE as f32;
        let alpha = omega.sin() / 2.0;
        let cosine = -2.0 * omega.cos();
        let denominator = 1.0 + alpha / amplitude;
        self.coefficients = [
            (1.0 + alpha * amplitude) / denominator,
            cosine / denominator,
            (1.0 - alpha * amplitude) / denominator,
            cosine / denominator,
            (1.0 - alpha / amplitude) / denominator,
        ];
    }

    fn process(&mut self, input: f32, channel: usize) -> f32 {
        if self.gain == 0.0 {
            return input;
        }
        let [b0, b1, b2, a1, a2] = self.coefficients;
        let state = &mut self.state[channel];
        let output = b0 * input + state[0];
        state[0] = b1 * input - a1 * output + state[1];
        state[1] = b2 * input - a2 * output;
        output
    }
}

#[derive(Default)]
pub struct Equalizer([Filter; BANDS.len()]);

impl Equalizer {
    pub fn configure(&mut self, gains: [f32; BANDS.len()]) {
        for ((filter, frequency), gain) in self.0.iter_mut().zip(BANDS).zip(gains) {
            filter.configure(frequency, gain);
        }
    }

    pub fn process(&mut self, mut sample: f32, channel: usize) -> f32 {
        for filter in &mut self.0 {
            sample = filter.process(sample, channel);
        }
        sample
    }
}
