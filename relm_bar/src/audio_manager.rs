//! Audio system management with improved error handling and caching

use alsa::mixer::{Mixer, Selem, SelemId};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::AppError;
use crate::error::Result;

/// Audio device information
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

/// Types of audio devices
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

/// Audio system manager with caching and improved error handling
#[derive(Debug)]
pub struct AudioManager {
    mixer: Option<Mixer>,
    devices: Vec<AudioDevice>,
    device_cache: HashMap<String, AudioDevice>,
    last_update: Instant,
    update_interval: Duration,
    last_error_time: Option<Instant>,
    error_count: usize,
    max_error_logs: usize,
}

#[allow(dead_code)]
impl AudioManager {
    /// Create a new audio manager
    pub fn new() -> Self {
        let mixer = Self::initialize_mixer();

        let mut manager = Self {
            mixer,
            devices: Vec::new(),
            device_cache: HashMap::new(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(500),
            last_error_time: None,
            error_count: 0,
            max_error_logs: 10,
        };

        // Initial device scan
        if let Err(e) = manager.refresh_devices() {
            error!("Failed to initialize audio devices: {}", e);
        }

        manager
    }

    /// Initialize ALSA mixer
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

    /// Refresh audio devices list
    pub fn refresh_devices(&mut self) -> Result<()> {
        let mixer = self
            .mixer
            .as_ref()
            .ok_or_else(|| AppError::audio("No mixer available"))?;

        let mut new_devices = Vec::new();
        let mut device_index = 0;

        for selem in mixer.iter().filter_map(|elem| Selem::new(elem)) {
            let name = selem
                .get_id()
                .get_name()
                .map_err(|e| AppError::audio(format!("Failed to get element name: {}", e)))?
                .to_string();

            let has_playback_volume = selem.has_playback_volume();
            let has_playback_switch = selem.has_playback_switch();

            // Skip devices without any controls
            if !has_playback_volume && !has_playback_switch {
                continue;
            }

            let device_type = AudioDeviceType::from_name(&name);

            // Get current state
            let volume = if has_playback_volume {
                self.get_element_volume(&selem).unwrap_or(0)
            } else {
                0
            };

            let is_muted = if has_playback_switch {
                self.get_element_mute_status(&selem).unwrap_or(false)
            } else {
                false
            };

            let device = AudioDevice {
                name: name.clone(),
                index: device_index,
                volume,
                is_muted,
                has_volume_control: has_playback_volume,
                has_switch_control: has_playback_switch,
                description: device_type.description().to_string(),
                device_type,
            };

            new_devices.push(device.clone());
            self.device_cache.insert(name, device);
            device_index += 1;
        }

        self.devices = new_devices;
        self.last_update = Instant::now();
        self.error_count = 0; // Reset error count on successful refresh

        debug!("Refreshed {} audio devices", self.devices.len());
        Ok(())
    }

    /// Get all available audio devices
    pub fn get_devices(&self) -> &[AudioDevice] {
        &self.devices
    }

    /// Find device by name
    pub fn find_device(&self, name: &str) -> Option<&AudioDevice> {
        self.devices.iter().find(|dev| dev.name == name)
    }

    /// Get device by index
    pub fn get_device_by_index(&self, index: usize) -> Option<&AudioDevice> {
        self.devices.get(index)
    }

    /// Get master audio device
    pub fn get_master_device(&self) -> Option<&AudioDevice> {
        // Try to find Master device first
        if let Some(device) = self
            .devices
            .iter()
            .find(|dev| matches!(dev.device_type, AudioDeviceType::Master))
        {
            return Some(device);
        }

        // Fall back to first device with volume control
        self.devices.iter().find(|dev| dev.has_volume_control)
    }

    /// Set device volume and mute state
    pub fn set_volume(&mut self, device_name: &str, volume: i32, mute: bool) -> Result<()> {
        let mixer = self
            .mixer
            .as_ref()
            .ok_or_else(|| AppError::audio("No mixer available"))?;

        let elem_id = SelemId::new(device_name, 0);
        let selem = mixer
            .find_selem(&elem_id)
            .ok_or_else(|| AppError::audio(format!("Audio device '{}' not found", device_name)))?;

        // Set volume if supported
        if selem.has_playback_volume() {
            let (min, max) = selem.get_playback_volume_range();
            let alsa_volume = min + (max - min) * volume.clamp(0, 100) as i64 / 100;

            for &channel in &[
                alsa::mixer::SelemChannelId::FrontLeft,
                alsa::mixer::SelemChannelId::FrontRight,
            ] {
                selem
                    .set_playback_volume(channel, alsa_volume)
                    .map_err(|e| AppError::audio(format!("Failed to set volume: {}", e)))?;
            }
        }

        // Set mute state if supported
        if selem.has_playback_switch() {
            let switch_val = if mute { 0 } else { 1 };

            for &channel in &[
                alsa::mixer::SelemChannelId::FrontLeft,
                alsa::mixer::SelemChannelId::FrontRight,
            ] {
                selem
                    .set_playback_switch(channel, switch_val)
                    .map_err(|e| AppError::audio(format!("Failed to set mute state: {}", e)))?;
            }
        }

        // Update cached device state
        if let Some(cached_device) = self.device_cache.get_mut(device_name) {
            cached_device.volume = volume;
            cached_device.is_muted = mute;
        }

        // Update device in main list
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

    /// Toggle device mute state
    pub fn toggle_mute(&mut self, device_name: &str) -> Result<bool> {
        let current_state = self
            .find_device(device_name)
            .ok_or_else(|| AppError::audio(format!("Device '{}' not found", device_name)))?;

        let new_mute_state = !current_state.is_muted;
        self.set_volume(device_name, current_state.volume, new_mute_state)?;

        Ok(new_mute_state)
    }

    /// Adjust volume by step
    pub fn adjust_volume(&mut self, device_name: &str, step: i32) -> Result<i32> {
        let current_device = self
            .find_device(device_name)
            .ok_or_else(|| AppError::audio(format!("Device '{}' not found", device_name)))?;

        let new_volume = (current_device.volume + step).clamp(0, 100);
        self.set_volume(device_name, new_volume, current_device.is_muted)?;

        Ok(new_volume)
    }

    /// Update devices if needed (rate-limited)
    pub fn update_if_needed(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_update) > self.update_interval {
            if let Err(e) = self.refresh_devices() {
                self.handle_error(e);
                return false;
            }
            return true;
        }
        false
    }

    /// Get element volume as percentage
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
            .map_err(|e| AppError::audio(format!("Failed to get volume: {}", e)))?;

        Ok(((volume - min) * 100 / (max - min)) as i32)
    }

    /// Get element mute status
    fn get_element_mute_status(&self, selem: &Selem) -> Result<bool> {
        if !selem.has_playback_switch() {
            return Ok(false);
        }

        let switch = selem
            .get_playback_switch(alsa::mixer::SelemChannelId::FrontLeft)
            .map_err(|e| AppError::audio(format!("Failed to get switch state: {}", e)))?;

        Ok(switch == 0)
    }

    /// Handle audio system errors with rate limiting
    fn handle_error(&mut self, error: AppError) {
        let now = Instant::now();

        // Rate limit error logging
        if let Some(last_error_time) = self.last_error_time {
            if now.duration_since(last_error_time) < Duration::from_secs(5) {
                self.error_count += 1;
                if self.error_count > self.max_error_logs {
                    return; // Skip logging
                }
            } else {
                self.error_count = 1; // Reset counter
            }
        } else {
            self.error_count = 1;
        }

        self.last_error_time = Some(now);

        if self.error_count <= self.max_error_logs {
            error!("Audio system error: {}", error);
        } else if self.error_count == self.max_error_logs + 1 {
            warn!("Audio error rate limit reached, suppressing further errors");
        }
    }

    /// Get audio device statistics
    pub fn get_stats(&self) -> AudioStats {
        let total_devices = self.devices.len();
        let devices_with_volume = self.devices.iter().filter(|d| d.has_volume_control).count();
        let devices_with_switch = self.devices.iter().filter(|d| d.has_switch_control).count();
        let muted_devices = self.devices.iter().filter(|d| d.is_muted).count();

        AudioStats {
            total_devices,
            devices_with_volume,
            devices_with_switch,
            muted_devices,
            last_update: self.last_update,
        }
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Audio system statistics
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AudioStats {
    pub total_devices: usize,
    pub devices_with_volume: usize,
    pub devices_with_switch: usize,
    pub muted_devices: usize,
    pub last_update: Instant,
}
