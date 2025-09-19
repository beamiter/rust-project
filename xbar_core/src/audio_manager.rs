//! Audio system management with anyhow for error handling and a simplified, robust state model.

use alsa::mixer::{Mixer, Selem, SelemId};
use anyhow::{Context, Result, anyhow};
use log::{debug, error, info, warn};
use std::time::{Duration, Instant};

/// Audio device information.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDevice {
    pub name: String,
    pub index: usize,
    pub volume: i32,
    pub is_muted: bool,
    pub has_volume_control: bool,
    pub has_switch_control: bool,
    pub description: String,
    pub device_type: AudioDeviceType,
}

/// Types of audio devices.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioDeviceType {
    Master,
    Headphone,
    Speaker,
    Microphone,
    LineIn,
    Other(String),
}

impl AudioDeviceType {
    /// Determines the device type from its name.
    fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "master" => Self::Master,
            "headphone" | "headphones" => Self::Headphone,
            "speaker" | "speakers" => Self::Speaker,
            "mic" | "microphone" | "capture" | "internal mic" => Self::Microphone,
            "line" | "line in" | "line-in" => Self::LineIn,
            _ => Self::Other(name.to_string()),
        }
    }

    /// Provides a human-readable description for the device type.
    fn description(&self) -> &str {
        match self {
            Self::Master => "主音量",
            Self::Headphone => "耳机",
            Self::Speaker => "扬声器",
            Self::Microphone => "麦克风",
            Self::LineIn => "线路输入",
            Self::Other(name) => name,
        }
    }
}

/// Audio system manager with a single source of truth for device state.
#[derive(Debug)]
pub struct AudioManager {
    mixer: Option<Mixer>,
    /// The single, authoritative list of audio devices.
    devices: Vec<AudioDevice>,
    last_update: Instant,
    update_interval: Duration,
    last_error_time: Option<Instant>,
    error_count: usize,
    max_error_logs: usize,
}

impl AudioManager {
    /// Creates a new audio manager and performs an initial device scan.
    pub fn new() -> Self {
        let mixer = Self::initialize_mixer();

        let mut manager = Self {
            mixer,
            devices: Vec::new(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(500),
            last_error_time: None,
            error_count: 0,
            max_error_logs: 10,
        };

        if let Err(e) = manager.refresh_devices() {
            // Using {:?} with anyhow prints the full error chain with context.
            error!("Failed to initialize audio devices: {:?}", e);
        }

        manager
    }

    /// Initializes the ALSA mixer.
    fn initialize_mixer() -> Option<Mixer> {
        match Mixer::new("default", false) {
            Ok(mixer) => {
                info!("Successfully initialized ALSA mixer");
                Some(mixer)
            }
            Err(e) => {
                error!("Failed to initialize ALSA mixer: {}", e);
                None
            }
        }
    }

    /// Refreshes the list of audio devices from the system.
    pub fn refresh_devices(&mut self) -> Result<()> {
        let mixer = self
            .mixer
            .as_ref()
            .ok_or_else(|| anyhow!("Mixer not available during device refresh"))?;

        let mut new_devices = Vec::new();

        for (device_index, selem) in mixer.iter().filter_map(Selem::new).enumerate() {
            let name = selem
                .get_id()
                .get_name()
                .context("Failed to get element name")?
                .to_string();

            let has_playback_volume = selem.has_playback_volume();
            let has_playback_switch = selem.has_playback_switch();

            if !has_playback_volume && !has_playback_switch {
                continue;
            }

            let device_type = AudioDeviceType::from_name(&name);
            let volume = self.get_element_volume(&selem).unwrap_or(0);
            let is_muted = self.get_element_mute_status(&selem).unwrap_or(false);

            new_devices.push(AudioDevice {
                name,
                index: device_index,
                volume,
                is_muted,
                has_volume_control: has_playback_volume,
                has_switch_control: has_playback_switch,
                description: device_type.description().to_string(),
                device_type,
            });
        }

        self.devices = new_devices;
        self.last_update = Instant::now();
        self.error_count = 0; // Reset error count on successful refresh

        debug!("Refreshed {} audio devices", self.devices.len());
        Ok(())
    }

    /// Returns a slice of all available audio devices.
    pub fn get_devices(&self) -> &[AudioDevice] {
        &self.devices
    }

    /// Finds a device by its name.
    pub fn find_device(&self, name: &str) -> Option<&AudioDevice> {
        self.devices.iter().find(|dev| dev.name == name)
    }

    /// Gets a device by its index.
    pub fn get_device_by_index(&self, index: usize) -> Option<&AudioDevice> {
        self.devices.get(index)
    }

    /// Gets the master audio device, falling back to the first available device with volume control.
    pub fn get_master_device(&self) -> Option<&AudioDevice> {
        self.devices
            .iter()
            .find(|dev| matches!(dev.device_type, AudioDeviceType::Master))
            .or_else(|| self.devices.iter().find(|dev| dev.has_volume_control))
    }

    /// Sets the volume and mute state for a given device.
    pub fn set_volume(&mut self, device_name: &str, volume: i32, mute: bool) -> Result<()> {
        let mixer = self
            .mixer
            .as_ref()
            .ok_or_else(|| anyhow!("No mixer available to set volume"))?;

        let selem = mixer
            .find_selem(&SelemId::new(device_name, 0))
            .ok_or_else(|| anyhow!("Audio device '{}' not found", device_name))?;

        if selem.has_playback_volume() {
            let (min, max) = selem.get_playback_volume_range();
            let alsa_volume = min + (max - min) * volume.clamp(0, 100) as i64 / 100;
            selem
                .set_playback_volume_all(alsa_volume)
                .with_context(|| format!("Failed to set volume for '{}'", device_name))?;
        }

        if selem.has_playback_switch() {
            let switch_val = if mute { 0 } else { 1 };
            selem
                .set_playback_switch_all(switch_val)
                .with_context(|| format!("Failed to set mute for '{}'", device_name))?;
        }

        // Update the state in our single source of truth.
        if let Some(device) = self.devices.iter_mut().find(|d| d.name == device_name) {
            device.volume = volume;
            device.is_muted = mute;
        }

        info!(
            "Set '{}' volume to {}%, muted: {}",
            device_name, volume, mute
        );
        Ok(())
    }

    /// Toggles the mute state of a device.
    pub fn toggle_mute(&mut self, device_name: &str) -> Result<bool> {
        let current_state = self
            .find_device(device_name)
            .cloned() // Clone to avoid mutable/immutable borrow issues.
            .ok_or_else(|| anyhow!("Device '{}' not found for toggling mute", device_name))?;

        let new_mute_state = !current_state.is_muted;
        self.set_volume(device_name, current_state.volume, new_mute_state)?;

        Ok(new_mute_state)
    }

    /// Adjusts the volume of a device by a given step.
    pub fn adjust_volume(&mut self, device_name: &str, step: i32) -> Result<i32> {
        let current_device = self
            .find_device(device_name)
            .cloned() // Clone to avoid mutable/immutable borrow issues.
            .ok_or_else(|| anyhow!("Device '{}' not found for adjusting volume", device_name))?;

        let new_volume = (current_device.volume + step).clamp(0, 100);
        self.set_volume(device_name, new_volume, current_device.is_muted)?;

        Ok(new_volume)
    }

    /// Refreshes devices if the update interval has passed.
    pub fn update_if_needed(&mut self) -> bool {
        if self.last_update.elapsed() > self.update_interval {
            if let Err(e) = self.refresh_devices() {
                self.handle_error(e);
                return false;
            }
            return true;
        }
        false
    }

    /// Gets the volume of an element as a percentage (0-100).
    fn get_element_volume(&self, selem: &Selem) -> Result<i32> {
        if !selem.has_playback_volume() {
            return Ok(0);
        }

        let (min, max) = selem.get_playback_volume_range();
        if min == max {
            return Ok(0);
        }

        let volume = selem
            .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
            .context("Failed to get playback volume")?;

        Ok(((volume - min) * 100 / (max - min)) as i32)
    }

    /// Gets the mute status of an element.
    fn get_element_mute_status(&self, selem: &Selem) -> Result<bool> {
        if !selem.has_playback_switch() {
            return Ok(false);
        }

        let switch = selem
            .get_playback_switch(alsa::mixer::SelemChannelId::FrontLeft)
            .context("Failed to get playback switch state")?;

        Ok(switch == 0)
    }

    /// Handles audio system errors with rate limiting for logging.
    fn handle_error(&mut self, error: anyhow::Error) {
        let now = Instant::now();
        let should_log = if let Some(last_error_time) = self.last_error_time {
            if now.duration_since(last_error_time) < Duration::from_secs(5) {
                self.error_count += 1;
                self.error_count <= self.max_error_logs
            } else {
                self.error_count = 1;
                true
            }
        } else {
            self.error_count = 1;
            true
        };

        self.last_error_time = Some(now);

        if should_log {
            error!("Audio system error: {:?}", error);
        } else if self.error_count == self.max_error_logs + 1 {
            warn!("Audio error rate limit reached, suppressing further errors");
        }
    }

    /// Gets statistics about the current audio devices.
    pub fn get_stats(&self) -> AudioStats {
        AudioStats {
            total_devices: self.devices.len(),
            devices_with_volume: self.devices.iter().filter(|d| d.has_volume_control).count(),
            devices_with_switch: self.devices.iter().filter(|d| d.has_switch_control).count(),
            muted_devices: self.devices.iter().filter(|d| d.is_muted).count(),
            last_update: self.last_update,
        }
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the audio system.
#[derive(Debug, Clone)]
pub struct AudioStats {
    pub total_devices: usize,
    pub devices_with_volume: usize,
    pub devices_with_switch: usize,
    pub muted_devices: usize,
    pub last_update: Instant,
}
