use std::path::PathBuf;

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum Action {
    #[default]
    SongOnTime,
    SongAfterTrack,
    Clip,
}

#[derive(Deserialize, Serialize)]
pub struct Event {
    pub path: PathBuf,
    hour: u32,
    minute: u32,
    days: [bool; 7],
    enabled: bool,
    #[serde(default)]
    pub action: Action,
    #[serde(default)]
    once: Option<String>,
    #[serde(default)]
    last_run: Option<NaiveDate>,
}

impl Event {
    pub fn new(path: PathBuf) -> Self {
        let now = chrono::Local::now() + chrono::TimeDelta::minutes(1);
        Self {
            path,
            hour: now.hour(),
            minute: now.minute(),
            days: [true; 7],
            enabled: true,
            action: Action::default(),
            once: None,
            last_run: None,
        }
    }

    pub fn take_if_due(&mut self, date: NaiveDate, time: NaiveTime) -> bool {
        if !self.enabled
            || !self.matches_date(date)
            || (self.hour, self.minute) != (time.hour(), time.minute())
            || self.last_run == Some(date)
        {
            return false;
        }
        self.last_run = Some(date);
        true
    }

    fn matches_date(&self, date: NaiveDate) -> bool {
        match &self.once {
            Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d").ok() == Some(date),
            None => self.days[date.weekday().num_days_from_monday() as usize],
        }
    }

    pub fn next(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        if !self.enabled {
            return None;
        }
        let time = NaiveTime::from_hms_opt(self.hour, self.minute, 0)?;
        let future = |date: NaiveDate| {
            let next = date.and_time(time);
            (next >= now && self.last_run != Some(date)).then_some(next)
        };
        if let Some(date) = &self.once {
            return future(NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?);
        }
        (0..=7)
            .filter_map(|offset| now.date().checked_add_days(chrono::Days::new(offset)))
            .filter(|date| self.matches_date(*date))
            .find_map(future)
    }

    pub fn edit(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("action")
            .selected_text(match self.action {
                Action::Clip => "Clip",
                Action::SongOnTime => "Song · on time",
                Action::SongAfterTrack => "Song · after current track",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.action, Action::Clip, "Clip");
                ui.selectable_value(&mut self.action, Action::SongOnTime, "Song · on time");
                ui.selectable_value(
                    &mut self.action,
                    Action::SongAfterTrack,
                    "Song · after current track",
                );
            });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.enabled, "Enabled");
            let hour = ui.add(
                egui::DragValue::new(&mut self.hour)
                    .range(0..=23)
                    .min_decimals(0),
            );
            ui.label(":");
            let minute = ui.add(
                egui::DragValue::new(&mut self.minute)
                    .range(0..=59)
                    .min_decimals(0),
            );
            if hour.changed() || minute.changed() {
                self.last_run = None;
            }
        });
        let mut once = self.once.is_some();
        if ui.checkbox(&mut once, "One-time date").changed() {
            self.once = once.then(|| chrono::Local::now().format("%Y-%m-%d").to_string());
        }
        if let Some(date) = &mut self.once {
            ui.add(egui::TextEdit::singleline(date).hint_text("YYYY-MM-DD"));
            if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                ui.label("Enter a date as YYYY-MM-DD.");
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                for (day, label) in self
                    .days
                    .iter_mut()
                    .zip(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"])
                {
                    ui.checkbox(day, label);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_runs_once_per_day_and_respects_weekdays() {
        let mut event = Event::new("jingle.wav".into());
        event.hour = 12;
        event.minute = 0;
        event.days = [true, false, false, false, false, false, false];
        event.last_run = None;
        let monday = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        assert!(event.take_if_due(monday, noon));
        assert!(!event.take_if_due(monday, noon));
        assert!(!event.take_if_due(monday.succ_opt().unwrap(), noon));
        assert!(event.take_if_due(monday + chrono::Days::new(7), noon));
    }
}
