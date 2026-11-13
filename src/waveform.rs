use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::Duration,
};

use eframe::egui;
use rodio::{Decoder, Source};

pub struct Analysis {
    peaks: Vec<f32>,
    pub duration: Duration,
}

pub struct Waveform {
    pending: Receiver<Option<Analysis>>,
    cancelled: Arc<AtomicBool>,
    pub analysis: Option<Analysis>,
}

impl Waveform {
    pub fn start(path: PathBuf, context: egui::Context) -> Self {
        let (sender, pending) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        thread::spawn(move || {
            let _ = sender.send(analyze(path, &stop));
            context.request_repaint();
        });
        Self {
            pending,
            cancelled,
            analysis: None,
        }
    }

    pub fn poll(&mut self) {
        if let Ok(analysis) = self.pending.try_recv() {
            self.analysis = analysis;
        }
    }
}

impl Drop for Waveform {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

fn analyze(path: PathBuf, cancelled: &AtomicBool) -> Option<Analysis> {
    let source = Decoder::try_from(File::open(path).ok()?).ok()?;
    let samples_per_second = u64::from(source.sample_rate()) * u64::from(source.channels());
    let mut samples_per_peak = (samples_per_second / 100).max(1);
    let mut peaks = Vec::<f32>::with_capacity(2048);
    let mut count = 0_u64;
    let mut pending = 0_u64;
    let mut peak = 0.0_f32;

    for sample in source {
        peak = peak.max(sample.abs());
        count += 1;
        pending += 1;
        if pending == samples_per_peak {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            peaks.push(peak);
            pending = 0;
            peak = 0.0;
            if peaks.len() == 2048 {
                for index in 0..1024 {
                    peaks[index] = peaks[index * 2].max(peaks[index * 2 + 1]);
                }
                peaks.truncate(1024);
                samples_per_peak *= 2;
            }
        }
    }

    if pending > 0 {
        peaks.push(peak);
    }
    (count > 0).then(|| Analysis {
        peaks,
        duration: Duration::from_secs_f64(count as f64 / samples_per_second as f64),
    })
}

pub fn display(
    ui: &mut egui::Ui,
    analysis: Option<&Analysis>,
    progress: f32,
    accent: egui::Color32,
) -> Option<f32> {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    if rect.width() <= 0.0 {
        return None;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(6, 17, 12));
    let cursor = egui::lerp(rect.x_range(), progress);
    if let Some(analysis) = analysis {
        let columns = (rect.width() / 2.0).max(1.0) as usize;
        let muted = accent.gamma_multiply(0.36);
        for column in 0..columns {
            let start = column * analysis.peaks.len() / columns;
            let end = ((column + 1) * analysis.peaks.len() / columns).max(start + 1);
            let peak = analysis.peaks[start..end]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);
            let x = egui::lerp(rect.x_range(), (column as f32 + 0.5) / columns as f32);
            let height = (peak.min(1.0) * 14.0).max(0.5);
            painter.vline(
                x,
                rect.center().y - height..=rect.center().y + height,
                egui::Stroke::new(1.0_f32, if x <= cursor { accent } else { muted }),
            );
        }
    } else {
        painter.hline(
            rect.left()..=cursor,
            rect.center().y,
            egui::Stroke::new(2.0_f32, accent),
        );
    }
    painter.vline(cursor, rect.y_range(), egui::Stroke::new(1.0_f32, accent));

    if response.clicked() {
        response
            .interact_pointer_pos()
            .map(|pointer| ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
    } else {
        None
    }
}
