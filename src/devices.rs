use cpal::{
    Device,
    traits::{DeviceTrait, HostTrait},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    name: String,
    occurrence: usize,
}

impl Selection {
    pub fn label(&self) -> String {
        if self.occurrence == 0 {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.occurrence + 1)
        }
    }
}

#[derive(Clone, Copy)]
pub enum Direction {
    Input,
    Output,
}

impl Direction {
    fn devices(self) -> Result<Vec<(Selection, Device)>, String> {
        let host = cpal::default_host();
        let devices: Vec<_> = match self {
            Self::Input => host
                .input_devices()
                .map_err(|error| error.to_string())?
                .collect(),
            Self::Output => host
                .output_devices()
                .map_err(|error| error.to_string())?
                .collect(),
        };
        let mut counts = std::collections::HashMap::new();
        Ok(devices
            .into_iter()
            .filter_map(|device| {
                let name = device.name().ok()?;
                let occurrence = counts.entry(name.clone()).or_insert(0);
                let selection = Selection {
                    name,
                    occurrence: *occurrence,
                };
                *occurrence += 1;
                Some((selection, device))
            })
            .collect())
    }

    pub fn options(self) -> Result<Vec<Selection>, String> {
        Ok(self
            .devices()?
            .into_iter()
            .map(|(selection, _)| selection)
            .collect())
    }

    pub fn resolve(self, selection: Option<&Selection>) -> Result<Device, String> {
        if let Some(selection) = selection {
            return self
                .devices()?
                .into_iter()
                .find(|(candidate, _)| candidate == selection)
                .map(|(_, device)| device)
                .ok_or_else(|| format!("Audio device unavailable: {}", selection.label()));
        }
        let host = cpal::default_host();
        match self {
            Self::Input => host.default_input_device(),
            Self::Output => host.default_output_device(),
        }
        .ok_or_else(|| match self {
            Self::Input => "No default input device is available.".into(),
            Self::Output => "No default output device is available.".into(),
        })
    }
}
