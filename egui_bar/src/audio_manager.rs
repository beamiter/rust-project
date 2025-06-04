use alsa::mixer::{Mixer, Selem, SelemId};
use log::{error, info, warn};
use std::time::{Duration, Instant};

/// 表示一个音频设备控制器
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub index: u32,
    pub volume: i32, // 0-100 百分比
    pub is_muted: bool,
    pub has_volume_control: bool,
    pub has_switch_control: bool,
    pub description: String, // 友好名称
}

/// 音频管理器，处理所有与音频相关的操作
pub struct AudioManager {
    mixer: Option<Mixer>,
    devices: Vec<AudioDevice>,
    last_update: Instant,
    update_interval: Duration,
}

impl AudioManager {
    /// 创建新的音频管理器实例
    pub fn new() -> Self {
        let mixer = match Mixer::new("default", false) {
            Ok(mixer) => {
                info!("Successfully opened ALSA mixer");
                Some(mixer)
            }
            Err(e) => {
                error!("Failed to open ALSA mixer: {}", e);
                None
            }
        };

        let mut manager = Self {
            mixer,
            devices: Vec::new(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(500),
        };

        // 初始加载设备列表
        if let Err(e) = manager.refresh_devices() {
            error!("Error initializing audio devices: {}", e);
        }

        manager
    }

    /// 刷新音频设备列表和状态
    pub fn refresh_devices(&mut self) -> Result<(), String> {
        let mixer = match self.mixer.as_ref() {
            Some(m) => m,
            None => return Err("No mixer available".to_string()),
        };

        self.devices.clear();

        for selem in mixer
            .iter()
            .filter_map(|elem_enum_variant| Selem::new(elem_enum_variant))
        {
            let name_string = match selem.get_id().get_name() {
                Ok(ptr) => ptr.to_string(),
                Err(e) => {
                    warn!("Failed to get name for selem: {}", e);
                    continue; // 跳过无法获取名称的设备
                }
            };

            // 检查是否有控制能力
            let has_playback_volume = selem.has_playback_volume();
            let has_playback_switch = selem.has_playback_switch();
            let _has_capture_volume = selem.has_capture_volume();
            let _has_capture_switch = selem.has_capture_switch();

            // 跳过无回放控制能力的设备 (根据原逻辑，只关心 playback)
            if !has_playback_volume && !has_playback_switch {
                // 如果也关心 capture，则条件应为：
                // if !has_playback_volume && !has_playback_switch && !has_capture_volume && !has_capture_switch {
                continue;
            }

            // 获取音量
            let volume = if has_playback_volume {
                match self.get_element_volume(&selem) {
                    Ok(vol) => vol,
                    Err(e) => {
                        warn!("Failed to get volume for '{}': {}", name_string, e);
                        0 // 默认值
                    }
                }
            } else {
                0
            };

            // 获取静音状态
            let is_muted = if has_playback_switch {
                match self.get_element_mute_status(&selem) {
                    Ok(muted) => muted,
                    Err(e) => {
                        warn!("Failed to get mute status for '{}': {}", name_string, e);
                        false // 默认值
                    }
                }
            } else {
                false
            };

            // 确定设备描述
            let description = match name_string.as_str() {
                "Master" => "主音量".to_string(),
                "Headphone" => "耳机".to_string(),
                "Speaker" => "扬声器".to_string(),
                "Mic" | "Capture" | "Internal Mic" | "Internal Microphone" => "麦克风".to_string(),
                // 你可能需要添加更多常见的 ALSA simple element names
                _ => name_string.clone(), // 默认使用原始名称作为描述
            };

            // 添加到设备列表
            self.devices.push(AudioDevice {
                name: name_string, // name_string ownership 移到这里
                index: self.devices.len() as u32,
                volume,
                is_muted,
                has_volume_control: has_playback_volume,
                has_switch_control: has_playback_switch,
                description,
            });
        }

        self.last_update = Instant::now();
        info!(
            "Refreshed audio devices: found {} device(s)",
            self.devices.len()
        );
        Ok(())
    }

    /// 获取当前可用的音频设备列表
    pub fn get_devices(&self) -> &[AudioDevice] {
        &self.devices
    }

    /// 查找指定名称的设备
    pub fn find_device(&self, name: &str) -> Option<&AudioDevice> {
        self.devices.iter().find(|dev| dev.name == name)
    }

    /// 根据索引获取设备
    pub fn get_device_by_index(&self, index: usize) -> Option<&AudioDevice> {
        self.devices.get(index)
    }

    /// 获取主音量控制设备
    pub fn get_master_device(&self) -> Option<&AudioDevice> {
        // 优先查找 Master
        if let Some(device) = self.find_device("Master") {
            return Some(device);
        }

        // 其次查找第一个有音量控制的设备
        self.devices.iter().find(|dev| dev.has_volume_control)
    }

    /// 获取元素的音量百分比
    fn get_element_volume(&self, selem: &alsa::mixer::Selem) -> Result<i32, String> {
        if !selem.has_playback_volume() {
            return Ok(0);
        }

        let (min, max) = selem.get_playback_volume_range();
        if min == max {
            return Ok(0);
        }

        // 获取左声道音量
        let vol = selem
            .get_playback_volume(alsa::mixer::SelemChannelId::FrontLeft)
            .map_err(|e| format!("Failed to get volume: {}", e))?;

        // 转换为百分比
        Ok(((vol - min) * 100 / (max - min)) as i32)
    }

    /// 获取元素的静音状态
    fn get_element_mute_status(&self, selem: &alsa::mixer::Selem) -> Result<bool, String> {
        if !selem.has_playback_switch() {
            return Ok(false);
        }

        let switch = selem
            .get_playback_switch(alsa::mixer::SelemChannelId::FrontLeft)
            .map_err(|e| format!("Failed to get switch state: {}", e))?;

        Ok(switch == 0)
    }

    /// 设置设备音量
    pub fn set_volume(&mut self, device_name: &str, volume: i32, mute: bool) -> Result<(), String> {
        if self.mixer.is_none() {
            return Err("No mixer available".to_string());
        }

        let mixer = self.mixer.as_ref().unwrap();

        // 创建 SelemId
        let elem_id = SelemId::new(device_name, 0);

        // 获取元素
        let selem = mixer
            .find_selem(&elem_id)
            .ok_or_else(|| format!("Cannot find mixer element: {}", device_name))?;

        // 设置音量
        if selem.has_playback_volume() {
            let (min, max) = selem.get_playback_volume_range();
            let vol = min + (max - min) * volume as i64 / 100;

            // 设置所有声道
            for channel in &[
                alsa::mixer::SelemChannelId::FrontLeft,
                alsa::mixer::SelemChannelId::FrontRight,
            ] {
                if let Err(e) = selem.set_playback_volume(*channel, vol) {
                    return Err(format!("Failed to set volume: {}", e));
                }
            }
        }

        // 设置静音状态
        if selem.has_playback_switch() {
            let switch_val = if mute { 0 } else { 1 };

            // 设置所有声道
            for channel in &[
                alsa::mixer::SelemChannelId::FrontLeft,
                alsa::mixer::SelemChannelId::FrontRight,
            ] {
                if let Err(e) = selem.set_playback_switch(*channel, switch_val) {
                    return Err(format!("Failed to set mute state: {}", e));
                }
            }
        }

        // 更新本地缓存的设备状态
        if let Some(device) = self.devices.iter_mut().find(|d| d.name == device_name) {
            device.volume = volume;
            device.is_muted = mute;
        }

        info!("Set {} volume to {}%, muted: {}", device_name, volume, mute);
        Ok(())
    }

    /// 切换设备的静音状态
    pub fn toggle_mute(&mut self, device_name: &str) -> Result<bool, String> {
        // 获取当前状态
        let current_mute = self
            .find_device(device_name)
            .map(|dev| dev.is_muted)
            .unwrap_or(false);

        // 切换状态
        let new_mute = !current_mute;

        // 获取当前音量
        let volume = self
            .find_device(device_name)
            .map(|dev| dev.volume)
            .unwrap_or(50);

        // 设置新状态
        self.set_volume(device_name, volume, new_mute)?;

        Ok(new_mute)
    }

    /// 如果超过更新间隔，刷新设备信息
    pub fn update_if_needed(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_update) > self.update_interval {
            match self.refresh_devices() {
                Ok(_) => return true,
                Err(e) => {
                    error!("Failed to refresh audio devices: {}", e);
                    return false;
                }
            }
        }
        false
    }
}
