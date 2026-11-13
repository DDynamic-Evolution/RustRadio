#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod broadcast;
mod devices;
mod equalizer;
mod metadata;
mod microphone;
mod playlist;
mod recording;
mod rotation;
mod schedule;

use std::{collections::VecDeque, path::PathBuf, time::Duration};

use audio::{Audio, Deck};
use broadcast::Broadcast;
use eframe::egui;
use rotation::Rotation;
use schedule::{Action, Event};

const PLAYLIST_KEY: &str = "playlist";
const VOLUME_KEY: &str = "volume";

const LOGO: &[u8] = include_bytes!("../assets/logo.png");

fn main() -> eframe::Result {
    eframe::run_native(
        "RustRadio",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([850.0, 540.0])
                .with_icon(eframe::icon_data::from_png_bytes(LOGO).expect("embedded logo")),
            ..Default::default()
        },
        Box::new(|context| Ok(Box::new(Radio::new(context.storage)))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Audio,
    Equalizer,
    Automation,
    Streams,
    Recording,
}

struct PlaylistDrag(usize);

struct Radio {
    playlist: Vec<PathBuf>,
    selected: Option<usize>,
    current: Option<usize>,
    deck: Option<Deck>,
    jingles: Vec<PathBuf>,
    jingle: Option<(usize, Deck)>,
    audio: Option<Audio>,
    fade: Option<(Deck, Duration)>,
    fade_seconds: f32,
    transition_attempted: bool,
    rotating: bool,
    rotation: Rotation,
    events: Vec<Event>,
    pending_events: VecDeque<(PathBuf, Action)>,
    scheduled_clip: Option<Deck>,
    pending_clips: VecDeque<PathBuf>,
    scheduled_track: Option<PathBuf>,
    show_settings: bool,
    settings_tab: SettingsTab,
    mic: Option<microphone::Capture>,
    input_device: Option<devices::Selection>,
    output_device: Option<devices::Selection>,
    inputs: Vec<devices::Selection>,
    outputs: Vec<devices::Selection>,
    device_error: Option<String>,
    eq_gains: [f32; equalizer::BANDS.len()],
    logo: Option<egui::TextureHandle>,
    stream_profiles: Vec<broadcast::Settings>,
    selected_stream: usize,
    broadcast: Option<Broadcast>,
    recording_dir: Option<PathBuf>,
    recording: Option<recording::Recording>,
    last_recording: Option<PathBuf>,
    volume: f32,
    stream_gain: f32,
    mic_gain: f32,
    error: Option<String>,
}

impl Radio {
    fn new(storage: Option<&dyn eframe::Storage>) -> Self {
        let stream_profiles: Vec<broadcast::Settings> = storage
            .and_then(|storage| eframe::get_value(storage, "stream_profiles"))
            .filter(|profiles: &Vec<broadcast::Settings>| !profiles.is_empty())
            .unwrap_or_else(|| vec![saved(storage, "broadcast", broadcast::Settings::default())]);
        let selected_stream =
            saved::<usize>(storage, "selected_stream", 0).min(stream_profiles.len() - 1);
        Self {
            playlist: storage
                .and_then(|storage| eframe::get_value(storage, PLAYLIST_KEY))
                .unwrap_or_default(),
            volume: storage
                .and_then(|storage| eframe::get_value::<f32>(storage, VOLUME_KEY))
                .filter(|value| value.is_finite())
                .unwrap_or(0.8)
                .clamp(0.0, 1.0),
            selected: None,
            current: None,
            mic_gain: saved::<f32>(storage, "mic_gain", 1.0).clamp(0.0, 1.0),
            stream_gain: saved::<f32>(storage, "stream_gain", 1.0).clamp(0.0, 1.0),
            deck: None,
            jingles: saved(storage, "jingles", Vec::new()),
            jingle: None,
            audio: None,
            fade: None,
            fade_seconds: saved::<f32>(storage, "crossfade", 0.0).clamp(0.0, 15.0),
            transition_attempted: false,
            rotating: saved(storage, "rotation", false),
            rotation: Rotation::default(),
            events: saved(storage, "events", Vec::new()),
            pending_events: VecDeque::new(),
            scheduled_clip: None,
            pending_clips: VecDeque::new(),
            scheduled_track: None,
            show_settings: false,
            settings_tab: SettingsTab::Audio,
            mic: None,
            input_device: saved(storage, "input_device", None),
            output_device: saved(storage, "output_device", None),
            inputs: Vec::new(),
            outputs: Vec::new(),
            device_error: None,
            eq_gains: saved::<[f32; equalizer::BANDS.len()]>(
                storage,
                "equalizer",
                [0.0; equalizer::BANDS.len()],
            )
            .map(|gain| {
                if gain.is_finite() {
                    gain.clamp(-12.0, 12.0)
                } else {
                    0.0
                }
            }),
            logo: None,
            stream_profiles,
            selected_stream,
            broadcast: None,
            recording_dir: saved(storage, "recording_dir", None),
            recording: None,
            last_recording: None,
            error: None,
        }
    }

    fn audio(&mut self) -> Result<&Audio, String> {
        if self.audio.is_none() {
            let audio = Audio::open(self.output_device.as_ref())?;
            audio.set_eq(self.eq_gains);
            audio.set_stream_gain(self.stream_gain);
            audio.set_mic_gain(self.mic_gain);
            self.audio = Some(audio);
        }
        Ok(self.audio.as_ref().expect("audio initialized"))
    }

    fn start_path(&mut self, path: &std::path::Path, overlap: Duration) -> Result<(), String> {
        let gain = if overlap.is_zero() { self.volume } else { 0.0 };
        let next = self.audio()?.deck(path, gain)?;
        self.fade = self
            .deck
            .take()
            .filter(|_| !overlap.is_zero())
            .map(|deck| (deck, overlap));
        self.deck = Some(next);
        self.transition_attempted = false;
        Ok(())
    }

    fn start(&mut self, index: usize, overlap: Duration) -> Result<(), String> {
        let path = self.playlist[index].clone();
        self.start_path(&path, overlap)?;
        self.current = Some(index);
        self.scheduled_track = None;
        self.selected = Some(index);
        self.rotation.mark_played(index);
        self.error = None;
        Ok(())
    }

    fn play(&mut self, index: usize) {
        if let Err(error) = self.start(index, Duration::ZERO) {
            self.error = Some(error);
        }
    }

    fn stop(&mut self) {
        self.deck = None;
        self.fade = None;
        self.current = None;
        self.scheduled_track = None;
        self.pending_events.clear();
    }

    fn set_paused(&self, paused: bool) {
        for deck in self
            .deck
            .iter()
            .chain(self.fade.iter().map(|(deck, _)| deck))
        {
            if paused {
                deck.player.pause();
            } else {
                deck.player.play();
            }
        }
    }

    fn candidates(&mut self) -> Vec<usize> {
        if self.rotating {
            self.rotation.candidates(self.playlist.len(), self.current)
        } else {
            let start = self.current.map_or(0, |index| index + 1);
            (start..self.playlist.len()).collect()
        }
    }

    fn advance(&mut self, overlap: Duration) {
        let mut skipped = Vec::new();
        for index in self.candidates() {
            match self.start(index, overlap) {
                Ok(()) => break,
                Err(error) => {
                    skipped.push(error);
                    if self.audio.is_none() {
                        break;
                    }
                }
            }
        }
        if !skipped.is_empty() {
            self.error = Some(skipped.join("\n"));
        }
    }

    fn tick(&mut self) {
        let now = chrono::Local::now();
        for event in &mut self.events {
            if event.take_if_due(now.date_naive(), now.time()) {
                match event.action {
                    Action::Clip => self.pending_clips.push_back(event.path.clone()),
                    action => self.pending_events.push_back((event.path.clone(), action)),
                }
            }
        }

        if self
            .scheduled_clip
            .as_ref()
            .is_some_and(|clip| clip.player.empty())
        {
            self.scheduled_clip = None;
        }
        while self.scheduled_clip.is_none() {
            let Some(path) = self.pending_clips.pop_front() else {
                break;
            };
            let gain = self.volume;
            match self.audio().and_then(|audio| audio.deck(&path, gain)) {
                Ok(clip) => self.scheduled_clip = Some(clip),
                Err(error) => self.error = Some(error),
            }
        }
        let music_volume = if let Some(clip) = &self.scheduled_clip {
            clip.player.set_volume(self.volume);
            self.volume * audio::DUCK_GAIN
        } else {
            self.volume
        };

        let ended = self.deck.as_ref().is_none_or(|deck| deck.player.empty());
        if ended || self.scheduled_track.is_none() {
            while let Some(index) = self
                .pending_events
                .iter()
                .position(|(_, action)| ended || *action == Action::SongOnTime)
            {
                let (path, _) = self.pending_events.remove(index).expect("queued event");
                match self.start_path(&path, Duration::ZERO) {
                    Ok(()) => {
                        self.scheduled_track = Some(path);
                        if let Some(deck) = &self.deck {
                            deck.player.set_volume(music_volume);
                        }
                        return;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }

        if self.deck.is_some() && ended {
            self.deck = None;
            self.fade = None;
            self.advance(Duration::ZERO);
            if self.deck.is_none() {
                self.stop();
            }
        }

        let Some(deck) = &self.deck else { return };
        if let Some((outgoing, duration)) = &self.fade {
            let progress = (deck.player.get_pos().as_secs_f32() / duration.as_secs_f32()).min(1.0);
            deck.player.set_volume(music_volume * progress);
            outgoing.player.set_volume(music_volume * (1.0 - progress));
            if progress >= 1.0 || outgoing.player.empty() {
                deck.player.set_volume(music_volume);
                self.fade = None;
            }
        } else {
            deck.player.set_volume(music_volume);
            if self.scheduled_track.is_none()
                && self.pending_events.is_empty()
                && !deck.player.is_paused()
                && !self.transition_attempted
                && self.fade_seconds > 0.0
                && let Some(remaining) = deck.remaining()
                && remaining > Duration::ZERO
                && remaining.as_secs_f32() <= self.fade_seconds
                && deck.player.get_pos() >= remaining
            {
                self.transition_attempted = true;
                self.advance(remaining);
            }
        }
    }

    fn stop_microphone(&mut self) {
        if let Some(audio) = &self.audio {
            audio.set_microphone(None);
        }
        self.mic = None;
    }

    fn toggle_microphone(&mut self) {
        if self.mic.is_some() {
            self.stop_microphone();
        } else {
            match self
                .audio()
                .map(|_| ())
                .and_then(|_| microphone::Capture::open(self.input_device.as_ref()))
            {
                Ok(capture) => {
                    self.audio
                        .as_ref()
                        .unwrap()
                        .set_microphone(Some(capture.input.clone()));
                    self.mic = Some(capture);
                }
                Err(error) => self.error = Some(format!("Microphone: {error}")),
            }
        }
    }

    fn refresh_devices(&mut self) {
        self.device_error = None;
        for (direction, options) in [
            (devices::Direction::Input, &mut self.inputs),
            (devices::Direction::Output, &mut self.outputs),
        ] {
            match direction.options() {
                Ok(devices) => *options = devices,
                Err(error) => {
                    options.clear();
                    self.device_error = Some(format!("Could not list audio devices: {error}"));
                }
            }
        }
    }

    fn playback_display(&self, ui: &mut egui::Ui) {
        let path = self
            .scheduled_track
            .as_ref()
            .or_else(|| self.current.and_then(|index| self.playlist.get(index)));
        let (status, title, elapsed, accent) = match (path, &self.deck) {
            (Some(path), Some(deck)) => {
                let (status, accent) = if deck.player.is_paused() {
                    ("PAUSED", egui::Color32::from_rgb(255, 207, 120))
                } else {
                    ("PLAYING", egui::Color32::from_rgb(152, 255, 174))
                };
                (
                    status,
                    track_name(path),
                    deck.player.get_pos().as_secs(),
                    accent,
                )
            }
            _ => (
                "STOPPED",
                "No track playing".into(),
                0,
                egui::Color32::from_rgb(164, 190, 172),
            ),
        };

        ui.add_space(4.0);
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(10, 26, 18))
            .stroke(egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgb(69, 105, 82),
            ))
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(14, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:02}:{:02}", elapsed / 60, elapsed % 60))
                            .monospace()
                            .size(24.0)
                            .strong()
                            .color(accent),
                    );
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.label(
                            egui::RichText::new(status)
                                .monospace()
                                .size(11.0)
                                .strong()
                                .color(accent),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&title)
                                    .monospace()
                                    .size(16.0)
                                    .color(egui::Color32::from_rgb(218, 245, 225)),
                            )
                            .truncate(),
                        )
                        .on_hover_text(title);
                    });
                });
            });
    }

    fn stream_level_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut percent = self.stream_gain * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut percent, 0.0..=100.0)
                        .integer()
                        .suffix("%")
                        .text("Stream volume"),
                )
                .changed()
            {
                self.stream_gain = percent / 100.0;
                if let Some(audio) = &self.audio {
                    audio.set_stream_gain(self.stream_gain);
                }
            }

            let peak = self.audio.as_ref().map_or(0.0, Audio::stream_peak);
            peak_meter(ui, "Stream peak", peak);
        });
    }

    fn microphone_level_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut percent = self.mic_gain * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut percent, 0.0..=100.0)
                        .integer()
                        .suffix("%")
                        .text("Mic volume"),
                )
                .changed()
            {
                self.mic_gain = percent / 100.0;
                if let Some(audio) = &self.audio {
                    audio.set_mic_gain(self.mic_gain);
                }
            }
            let peak = if self.mic.is_some() {
                self.audio.as_ref().map_or(0.0, Audio::mic_peak)
            } else {
                0.0
            };
            peak_meter(ui, "Mic peak", peak);
        });
    }

    fn settings_ui(&mut self, context: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .default_size([460.0, 450.0])
            .min_width(240.0)
            .resizable(true)
            .vscroll(true)
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (SettingsTab::Audio, "Audio"),
                        (SettingsTab::Equalizer, "Equalizer"),
                        (SettingsTab::Automation, "Automation"),
                        (SettingsTab::Streams, "Streams"),
                        (SettingsTab::Recording, "Recording"),
                    ] {
                        ui.selectable_value(&mut self.settings_tab, tab, label);
                    }
                });
                ui.separator();
                match self.settings_tab {
                    SettingsTab::Audio => {
                        ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).text("Volume"));
                        ui.separator();
                        self.devices_ui(ui);
                    }
                    SettingsTab::Equalizer => self.equalizer_ui(ui),
                    SettingsTab::Automation => {
                        ui.add(
                            egui::Slider::new(&mut self.fade_seconds, 0.0..=15.0)
                                .suffix(" s")
                                .text("Crossfade"),
                        );
                        ui.separator();
                    }
                    SettingsTab::Streams => self.broadcast_ui(ui),
                    SettingsTab::Recording => self.recording_settings(ui),
                }
            });
        self.show_settings = open;
    }

    fn recording_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("MP3 recording folder");
        if let Some(folder) = &self.recording_dir {
            ui.label(folder.display().to_string());
        }
        if ui.button("Choose folder").clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            self.recording_dir = Some(folder);
        }
        if let Some(path) = &self.last_recording {
            ui.label(format!("Last recording: {}", path.display()));
        }
    }

    fn start_recording(&mut self) {
        let Some(folder) = self.recording_dir.clone() else {
            self.settings_tab = SettingsTab::Recording;
            self.show_settings = true;
            return;
        };
        let result = self.audio().and_then(|audio| {
            let (recording, sender) = recording::Recording::start(&folder)
                .map_err(|error| format!("Could not start MP3 recording: {error}"))?;
            audio.set_recording_feed(Some(sender));
            Ok(recording)
        });
        match result {
            Ok(recording) => self.recording = Some(recording),
            Err(error) => self.error = Some(error),
        }
    }

    fn stop_recording(&mut self) {
        if let Some(recording) = self.recording.take() {
            if let Some(audio) = &self.audio {
                audio.set_recording_feed(None);
            }
            let path = recording.path.clone();
            match recording.finish() {
                Ok(()) => self.last_recording = Some(path),
                Err(error) => self.error = Some(format!("Recording {}: {error}", path.display())),
            }
        }
    }

    fn devices_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Refresh devices").clicked() {
            self.refresh_devices();
        }
        let input = device_picker(ui, "Microphone input", &self.input_device, &self.inputs);
        if input != self.input_device {
            let result = if self.mic.is_some() {
                microphone::Capture::open(input.as_ref()).map(Some)
            } else {
                Ok(None)
            };
            match result {
                Ok(capture) => {
                    if let Some(capture) = capture {
                        if let Some(audio) = &self.audio {
                            audio.set_microphone(Some(capture.input.clone()));
                        }
                        self.mic = Some(capture);
                    }
                    self.input_device = input;
                    self.device_error = None;
                }
                Err(error) => self.device_error = Some(format!("Microphone: {error}")),
            }
        }
        let output = device_picker(ui, "Playback output", &self.output_device, &self.outputs);
        if output != self.output_device {
            let result = self
                .audio
                .as_mut()
                .map_or(Ok(()), |audio| audio.change_output(output.as_ref()));
            match result {
                Ok(()) => {
                    self.output_device = output;
                    self.device_error = None;
                }
                Err(error) => self.device_error = Some(error),
            }
        }
        if let Some(error) = &self.device_error {
            ui.label(error);
        }
    }

    fn equalizer_ui(&mut self, ui: &mut egui::Ui) {
        let old_gains = self.eq_gains;
        ui.label("Music EQ");
        for (gain, label) in self
            .eq_gains
            .iter_mut()
            .zip(["80 Hz", "250 Hz", "1 kHz", "4 kHz", "12 kHz"])
        {
            ui.add(
                egui::Slider::new(gain, -12.0..=12.0)
                    .step_by(0.5)
                    .suffix(" dB")
                    .text(label),
            );
        }
        if ui.button("Reset to flat").clicked() {
            self.eq_gains.fill(0.0);
        }
        if self.eq_gains != old_gains
            && let Some(audio) = &self.audio
        {
            audio.set_eq(self.eq_gains);
        }
    }

    fn schedule_ui(&mut self, context: &egui::Context) {
        egui::SidePanel::right("scheduler")
            .default_width(250.0)
            .width_range(175.0..=420.0)
            .resizable(true)
            .show(context, |ui| {
                ui.heading("Scheduler");
                if ui.button("Add audio file").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file()
                {
                    self.events.push(Event::new(path));
                }
                let mut add = None;
                ui.horizontal_wrapped(|ui| {
                    for (label, paths, action) in [
                        ("From playlist", &self.playlist, Action::SongOnTime),
                        ("From jingles", &self.jingles, Action::Clip),
                    ] {
                        ui.add_enabled_ui(!paths.is_empty(), |ui| {
                            ui.menu_button(label, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(280.0)
                                    .show(ui, |ui| {
                                        for path in paths {
                                            if ui.button(track_name(path)).clicked() {
                                                add = Some((path.clone(), action));
                                                ui.close();
                                            }
                                        }
                                    });
                            });
                        });
                    }
                });
                if let Some((path, action)) = add {
                    let mut event = Event::new(path);
                    event.action = action;
                    self.events.push(event);
                }
                let now = chrono::Local::now().naive_local();
                if let Some(next) = self.events.iter().filter_map(|event| event.next(now)).min() {
                    ui.label(format!("Next: {}", next.format("%a %Y-%m-%d %H:%M")));
                }
                if !self.pending_events.is_empty() || !self.pending_clips.is_empty() {
                    ui.label(format!(
                        "Queued: {}",
                        self.pending_events.len() + self.pending_clips.len()
                    ));
                }
                if self.scheduled_clip.is_some() && ui.button("Stop scheduled clip").clicked() {
                    self.scheduled_clip = None;
                }
                let mut remove = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, event) in self.events.iter_mut().enumerate() {
                        ui.separator();
                        ui.push_id(index, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("×").on_hover_text("Remove event").clicked() {
                                    remove = Some(index);
                                }
                                ui.add(egui::Label::new(track_name(&event.path)).truncate())
                                    .on_hover_text(event.path.display().to_string());
                            });
                            event.edit(ui);
                        });
                    }
                });
                if let Some(index) = remove {
                    self.events.remove(index);
                }
            });
    }

    fn disconnect(&mut self) {
        if let Some(audio) = &self.audio {
            audio.set_feed(None);
        }
        self.broadcast = None;
    }

    fn broadcast_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(broadcast) = &self.broadcast {
            let status = format!("{} · {}", broadcast.name, broadcast.status());
            let disconnect = ui
                .horizontal_wrapped(|ui| {
                    ui.label(status);
                    ui.button("Disconnect").clicked()
                })
                .inner;
            if disconnect {
                self.disconnect();
            }
            ui.separator();
        }

        egui::ComboBox::from_id_salt("stream_profile")
            .width(ui.available_width())
            .selected_text(self.stream_profiles[self.selected_stream].label())
            .show_ui(ui, |ui| {
                for (index, profile) in self.stream_profiles.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_stream, index, profile.label());
                }
            });
        ui.horizontal_wrapped(|ui| {
            if ui.button("New profile").clicked() {
                let mut profile = broadcast::Settings::default();
                profile.name = format!("Stream {}", self.stream_profiles.len() + 1);
                self.stream_profiles.push(profile);
                self.selected_stream = self.stream_profiles.len() - 1;
            }
            if ui
                .add_enabled(
                    self.stream_profiles.len() > 1,
                    egui::Button::new("Delete profile"),
                )
                .clicked()
            {
                self.stream_profiles.remove(self.selected_stream);
                self.selected_stream = self.selected_stream.min(self.stream_profiles.len() - 1);
            }
        });
        ui.separator();
        ui.push_id(self.selected_stream, |ui| {
            self.stream_profiles[self.selected_stream].edit(ui)
        });
        ui.label("Profiles are saved automatically. Source passwords are kept for this session.");

        if ui
            .add_enabled(self.broadcast.is_none(), egui::Button::new("Connect"))
            .clicked()
        {
            self.connect_stream();
        }
    }

    fn connect_stream(&mut self) {
        let result = self
            .audio()
            .map(|_| ())
            .and_then(|_| Broadcast::start(&self.stream_profiles[self.selected_stream]));
        match result {
            Ok((broadcast, sender)) => {
                self.audio.as_ref().unwrap().set_feed(Some(sender));
                self.broadcast = Some(broadcast);
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn move_track(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        let track = self.playlist.remove(from);
        self.playlist.insert(to, track);
        self.selected = Some(to);
        self.current = self.current.map(|index| {
            if index == from {
                to
            } else if from < index && index <= to {
                index - 1
            } else if to <= index && index < from {
                index + 1
            } else {
                index
            }
        });
        self.rotation.reset(self.playlist.len(), self.current);
    }

    fn remove(&mut self, index: usize) {
        if self.current == Some(index) {
            self.stop();
        }
        self.playlist.remove(index);
        self.current = self
            .current
            .map(|current| current - usize::from(current > index));
        self.selected = self
            .playlist
            .len()
            .checked_sub(1)
            .map(|last| index.min(last));
        self.rotation.reset(self.playlist.len(), self.current);
    }

    fn play_jingle(&mut self, index: usize) {
        let path = self.jingles[index].clone();
        let gain = self.volume;
        match self.audio().and_then(|audio| audio.deck(&path, gain)) {
            Ok(deck) => self.jingle = Some((index, deck)),
            Err(error) => self.error = Some(error),
        }
    }

    fn remove_jingle(&mut self, index: usize) {
        self.jingles.remove(index);
        if let Some((playing, _)) = &mut self.jingle {
            match (*playing).cmp(&index) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => self.jingle = None,
                std::cmp::Ordering::Greater => *playing -= 1,
            }
        }
    }

    fn jingles_ui(&mut self, context: &egui::Context) {
        egui::SidePanel::right("jingles")
            .default_width(240.0)
            .width_range(160.0..=400.0)
            .resizable(true)
            .show(context, |ui| {
                ui.heading("Jingles");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Add clips").clicked()
                        && let Some(paths) = rfd::FileDialog::new()
                            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"])
                            .pick_files()
                    {
                        self.jingles.extend(paths);
                    }
                    if ui
                        .add_enabled(self.jingle.is_some(), egui::Button::new("Stop clip"))
                        .clicked()
                    {
                        self.jingle = None;
                    }
                });
                ui.separator();
                if self.jingles.is_empty() {
                    ui.label("Add jingles, station IDs, or sound effects.");
                }

                let mut play = None;
                let mut remove = None;
                egui::ScrollArea::vertical()
                    .id_salt("jingle_list")
                    .show(ui, |ui| {
                        for (index, path) in self.jingles.iter().enumerate() {
                            let playing = self
                                .jingle
                                .as_ref()
                                .is_some_and(|(active, _)| *active == index);
                            ui.horizontal(|ui| {
                                if ui.small_button("×").on_hover_text("Remove clip").clicked() {
                                    remove = Some(index);
                                }
                                let label =
                                    egui::Button::selectable(playing, track_name(path)).truncate();
                                if ui
                                    .add(label)
                                    .on_hover_text(path.display().to_string())
                                    .clicked()
                                {
                                    play = Some(index);
                                }
                            });
                        }
                    });
                if let Some(index) = play {
                    self.play_jingle(index);
                }
                if let Some(index) = remove {
                    self.remove_jingle(index);
                }
            });
    }

    fn load_playlist(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .add_filter("Playlist", &["m3u8", "m3u"])
            .pick_file()
        else {
            return;
        };
        match playlist::load(&file) {
            Ok(tracks) => {
                self.stop();
                self.playlist.clear();
                self.selected = None;
                self.import(tracks);
                self.rotation.reset(self.playlist.len(), None);
            }
            Err(error) => self.error = Some(format!("Could not load {}: {error}", file.display())),
        }
    }

    fn save_playlist(&mut self) {
        let Some(mut file) = rfd::FileDialog::new()
            .add_filter("M3U8 playlist", &["m3u8"])
            .set_file_name("playlist.m3u8")
            .save_file()
        else {
            return;
        };
        if file.extension().is_none() {
            file.set_extension("m3u8");
        }
        if let Err(error) = playlist::save(&file, &self.playlist) {
            self.error = Some(format!("Could not save {}: {error}", file.display()));
        }
    }

    fn import(&mut self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            if self.selected.is_none() {
                self.selected = Some(self.playlist.len());
            }
            self.playlist.extend(paths);
            self.rotation.reset(self.playlist.len(), self.current);
        }
    }
}

impl eframe::App for Radio {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_recording();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PLAYLIST_KEY, &self.playlist);
        eframe::set_value(storage, "jingles", &self.jingles);
        eframe::set_value(storage, VOLUME_KEY, &self.volume);
        eframe::set_value(storage, "stream_gain", &self.stream_gain);
        eframe::set_value(storage, "recording_dir", &self.recording_dir);
        eframe::set_value(storage, "mic_gain", &self.mic_gain);
        eframe::set_value(storage, "equalizer", &self.eq_gains);
        eframe::set_value(storage, "input_device", &self.input_device);
        eframe::set_value(storage, "output_device", &self.output_device);
        eframe::set_value(storage, "crossfade", &self.fade_seconds);
        eframe::set_value(storage, "rotation", &self.rotating);
        eframe::set_value(storage, "events", &self.events);
        eframe::set_value(storage, "stream_profiles", &self.stream_profiles);
        eframe::set_value(storage, "selected_stream", &self.selected_stream);
    }

    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick();
        if self
            .recording
            .as_ref()
            .is_some_and(recording::Recording::is_finished)
        {
            self.stop_recording();
        }
        if let Some(broadcast) = &mut self.broadcast {
            let path = self.scheduled_track.as_deref().or_else(|| {
                self.current
                    .and_then(|index| self.playlist.get(index))
                    .map(PathBuf::as_path)
            });
            broadcast.update_title(path);
        }
        if let Some((_, clip)) = &self.jingle {
            if clip.player.empty() {
                self.jingle = None;
            } else {
                clip.player.set_volume(self.volume);
                context.request_repaint_after(Duration::from_millis(50));
            }
        }
        if let Some(error) = self.mic.as_ref().and_then(microphone::Capture::take_error) {
            self.stop_microphone();
            self.error = Some(format!("Microphone: {error}"));
        }
        if let Some(broadcast) = &mut self.broadcast
            && let Err(error) = broadcast.poll()
        {
            self.disconnect();
            self.error = Some(error);
        }
        if self.deck.is_some() || self.scheduled_clip.is_some() {
            context.request_repaint_after(Duration::from_millis(20));
        } else if !self.events.is_empty()
            || self.broadcast.is_some()
            || self.mic.is_some()
            || self.recording.is_some()
        {
            context.request_repaint_after(Duration::from_millis(50));
        }

        let dropped = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        self.import(dropped);

        egui::TopBottomPanel::top("transport").show(context, |ui| {
            let logo = self.logo.get_or_insert_with(|| {
                let icon = eframe::icon_data::from_png_bytes(LOGO).expect("embedded logo");
                context.load_texture(
                    "RustRadio logo",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
            ui.horizontal(|ui| {
                ui.image((logo.id(), egui::vec2(32.0, 32.0)));
                ui.heading("RustRadio");
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Add files").clicked()
                    && let Some(paths) = rfd::FileDialog::new()
                        .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aac"])
                        .pick_files()
                {
                    self.import(paths);
                }

                if ui
                    .add_enabled(
                        self.deck.is_some() || !self.playlist.is_empty(),
                        egui::Button::new("Play"),
                    )
                    .clicked()
                {
                    if self
                        .deck
                        .as_ref()
                        .is_some_and(|deck| deck.player.is_paused())
                        && (self.scheduled_track.is_some() || self.selected == self.current)
                    {
                        self.set_paused(false);
                    } else if !self.playlist.is_empty() {
                        if self.rotating && self.deck.is_none() {
                            self.advance(Duration::ZERO);
                        } else {
                            self.play(self.selected.or(self.current).unwrap_or(0));
                        }
                    }
                }
                if ui
                    .add_enabled(self.deck.is_some(), egui::Button::new("Pause"))
                    .clicked()
                {
                    self.set_paused(true);
                }
                if ui
                    .add_enabled(self.deck.is_some(), egui::Button::new("Stop"))
                    .clicked()
                {
                    self.stop();
                }
                if ui
                    .checkbox(&mut self.rotating, "Shuffle")
                    .on_hover_text("Play the playlist in a random order.")
                    .changed()
                {
                    self.rotation.reset(self.playlist.len(), self.current);
                }
                let has_next = self.deck.is_some()
                    && (self.rotating && !self.playlist.is_empty()
                        || self.current.map_or(0, |index| index + 1) < self.playlist.len());
                if ui
                    .add_enabled(has_next, egui::Button::new("Next"))
                    .clicked()
                {
                    self.advance(Duration::ZERO);
                }
            });

            ui.horizontal_wrapped(|ui| {
                let mic_on = self.mic.is_some();
                let mic_label = if mic_on {
                    "Microphone on"
                } else {
                    "Microphone off"
                };
                if ui.selectable_label(mic_on, mic_label).clicked() {
                    self.toggle_microphone();
                }
                if ui.button("Settings").clicked() {
                    self.refresh_devices();
                    self.show_settings = true;
                }
                if let Some(broadcast) = &self.broadcast {
                    ui.label(format!("{} · {}", broadcast.name, broadcast.status()));
                    if let Some(error) = broadcast.metadata_error() {
                        ui.label(error);
                    }
                    if ui.button("Disconnect stream").clicked() {
                        self.disconnect();
                    }
                } else {
                    let profile = self.stream_profiles[self.selected_stream]
                        .label()
                        .to_owned();
                    if ui
                        .button("Connect")
                        .on_hover_text(format!("Connect to {profile}"))
                        .clicked()
                    {
                        self.connect_stream();
                    }
                    ui.label(profile);
                }
            });

            ui.horizontal_wrapped(|ui| {
                if let Some(recording) = &self.recording {
                    ui.label("Recording MP3")
                        .on_hover_text(recording.path.display().to_string());
                    if ui.button("Stop recording").clicked() {
                        self.stop_recording();
                    }
                } else if ui.button("Record MP3").clicked() {
                    self.start_recording();
                }
            });

            self.microphone_level_ui(ui);
            self.stream_level_ui(ui);

            self.playback_display(ui);
        });

        self.schedule_ui(context);
        self.jingles_ui(context);

        egui::CentralPanel::default().show(context, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Playlist");
                if ui
                    .button("Load playlist")
                    .on_hover_text("Replace the current playlist.")
                    .clicked()
                {
                    self.load_playlist();
                }
                if ui.button("Save playlist").clicked() {
                    self.save_playlist();
                }
            });
            if let Some(index) = self.selected {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(index > 0, egui::Button::new("Move up"))
                        .clicked()
                    {
                        self.move_track(index, index - 1);
                    }
                    if ui
                        .add_enabled(
                            index + 1 < self.playlist.len(),
                            egui::Button::new("Move down"),
                        )
                        .clicked()
                    {
                        let selected = self.selected.unwrap_or(index);
                        self.move_track(selected, selected + 1);
                    }
                    if ui.button("Remove").clicked() {
                        self.remove(self.selected.unwrap_or(index));
                    }
                });
            }
            if self.playlist.is_empty() {
                ui.label("Add audio files or drop them here.");
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut reorder = None;
                let mut play = None;
                for (index, path) in self.playlist.iter().enumerate() {
                    let marker = if self.current == Some(index) {
                        "▶ "
                    } else {
                        ""
                    };
                    let button = egui::Button::selectable(
                        self.selected == Some(index),
                        format!("{marker}{}. {}", index + 1, track_name(path)),
                    )
                    .sense(egui::Sense::click_and_drag())
                    .right_text("")
                    .truncate();
                    let row = ui
                        .add_sized([ui.available_width(), ui.spacing().interact_size.y], button)
                        .on_hover_text(path.display().to_string());
                    row.dnd_set_drag_payload(PlaylistDrag(index));
                    if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
                        let before = pointer.y < row.rect.center().y;
                        let slot = index + usize::from(!before);
                        if let Some(payload) = row.dnd_hover_payload::<PlaylistDrag>()
                            && payload.0 != slot
                            && payload.0 + 1 != slot
                        {
                            let y = if before {
                                row.rect.top()
                            } else {
                                row.rect.bottom()
                            };
                            ui.painter().hline(
                                row.rect.x_range(),
                                y,
                                ui.visuals().selection.stroke,
                            );
                        }
                        if let Some(payload) = row.dnd_release_payload::<PlaylistDrag>()
                            && payload.0 < self.playlist.len()
                        {
                            reorder = Some((payload.0, slot - usize::from(payload.0 < slot)));
                        }
                    }
                    if row.clicked() {
                        self.selected = Some(index);
                    }
                    if row.double_clicked() {
                        play = Some(index);
                    }
                }
                if let Some((from, to)) = reorder {
                    self.move_track(from, to);
                }
                if let Some(index) = play {
                    self.play(index);
                }
            });
        });

        self.settings_ui(context);

        if self.error.is_some() {
            egui::Window::new("Error")
                .collapsible(false)
                .show(context, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            ui.label(self.error.as_deref().unwrap_or_default());
                        });
                    if ui.button("Close").clicked() {
                        self.error = None;
                    }
                });
        }
    }
}

fn track_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn device_picker(
    ui: &mut egui::Ui,
    label: &str,
    current: &Option<devices::Selection>,
    options: &[devices::Selection],
) -> Option<devices::Selection> {
    let mut choice = current.clone();
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .width(ui.available_width())
        .selected_text(current.as_ref().map_or_else(
            || "System default".into(),
            |device| {
                if options.contains(device) {
                    device.label()
                } else {
                    format!("{} (unavailable)", device.label())
                }
            },
        ))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut choice, None, "System default");
            for device in options {
                ui.selectable_value(&mut choice, Some(device.clone()), device.label());
            }
        });
    choice
}

fn peak_meter(ui: &mut egui::Ui, name: &str, peak: f32) {
    let db = 20.0 * peak.log10();
    let level = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let label = if db < -60.0 {
        "−∞ dBFS".into()
    } else {
        format!("{db:.1} dBFS")
    };
    let color = if db >= -1.0 {
        egui::Color32::from_rgb(235, 82, 73)
    } else if db >= -6.0 {
        egui::Color32::from_rgb(224, 165, 64)
    } else {
        egui::Color32::from_rgb(46, 147, 104)
    };
    ui.label(name);
    ui.add(
        egui::ProgressBar::new(level)
            .desired_width(150.0)
            .fill(color)
            .text(label),
    );
}

fn saved<T: serde::de::DeserializeOwned>(
    storage: Option<&dyn eframe::Storage>,
    key: &str,
    default: T,
) -> T {
    storage
        .and_then(|storage| eframe::get_value(storage, key))
        .unwrap_or(default)
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[derive(Default)]
    struct MemoryStorage(std::collections::HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn migrates_existing_stream_and_restores_multiple_profiles() {
        let mut storage = MemoryStorage::default();
        let old = "(protocol:Icecast,host:\"localhost\",port:8000,mount:\"/live\",user:\"source\",tls:true)";
        eframe::Storage::set_string(&mut storage, "broadcast", old.into());
        let mut radio = Radio::new(Some(&storage));
        assert_eq!(radio.stream_profiles[0].label(), "Stream 1");
        assert!(ron_string(&radio.stream_profiles[0]).contains("/live"));
        let mut second = broadcast::Settings::default();
        second.name = "Ersatzserver".into();
        radio.stream_profiles.push(second);
        radio.selected_stream = 1;
        eframe::App::save(&mut radio, &mut storage);
        let restored = Radio::new(Some(&storage));
        assert_eq!(restored.stream_profiles.len(), 2);
        assert_eq!(restored.selected_stream, 1);
        assert_eq!(restored.stream_profiles[1].label(), "Ersatzserver");
    }

    #[test]
    fn reordering_keeps_the_playing_track_selected_by_its_new_index() {
        let mut radio = Radio::new(None);
        radio.playlist = ["a", "b", "c", "d"].map(PathBuf::from).to_vec();
        radio.current = Some(1);

        radio.move_track(0, 3);
        assert_eq!(radio.playlist, ["b", "c", "d", "a"].map(PathBuf::from));
        assert_eq!(radio.current, Some(0));
        assert_eq!(radio.selected, Some(3));

        radio.move_track(0, 2);
        assert_eq!(radio.playlist, ["c", "d", "b", "a"].map(PathBuf::from));
        assert_eq!(radio.current, Some(2));
    }

    fn ron_string(profile: &broadcast::Settings) -> String {
        let mut storage = MemoryStorage::default();
        eframe::set_value(&mut storage, "profile", profile);
        eframe::Storage::get_string(&storage, "profile").unwrap()
    }
}
