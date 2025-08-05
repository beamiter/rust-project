// config.rs
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use x11::keysym::*;
use x11::xlib::*;

use crate::dwm;
use crate::dwm::Button;
use crate::dwm::Dwm;
use crate::dwm::Key;
use crate::dwm::Rule;
use crate::terminal_prober::ADVANCED_TERMINAL_PROBER;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlConfig {
    pub appearance: AppearanceConfig,
    pub behavior: BehaviorConfig,
    pub status_bar: StatusBarConfig,
    pub colors: ColorsConfig,
    pub keybindings: KeyBindingsConfig,
    pub mouse_bindings: MouseBindingsConfig,
    pub rules: Vec<RuleConfig>,
    pub layout: LayoutConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    pub border_px: u32,
    pub snap: u32,
    pub dmenu_font: String,
    pub status_bar_pad: i32,
    pub broken: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorConfig {
    pub focus_follows_new_window: bool,
    pub resize_hints: bool,
    pub lock_fullscreen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusBarConfig {
    pub name: String,
    pub bar_0: String,
    pub bar_1: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorsConfig {
    pub dark_sea_green1: String,
    pub dark_sea_green2: String,
    pub pale_turquoise1: String,
    pub light_sky_blue1: String,
    pub grey84: String,
    pub cyan: String,
    pub white: String,
    pub black: String,
    pub transparent: u8,
    pub opaque: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub m_fact: f32,
    pub n_master: u32,
    pub tags_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindingsConfig {
    pub modkey: String, // "Mod1", "Mod4", etc.
    pub keys: Vec<KeyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyConfig {
    pub modifier: Vec<String>, // ["Mod1", "Shift"]
    pub key: String,           // "Return", "j", "k", etc.
    pub function: String,      // "spawn", "focusstack", etc.
    pub argument: ArgumentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentConfig {
    Int(i32),
    UInt(u32),
    Float(f32),
    String(String),
    StringVec(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseBindingsConfig {
    pub buttons: Vec<ButtonConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonConfig {
    pub click_type: String, // "ClkLtSymbol", "ClkWinTitle", etc.
    pub modifier: Vec<String>,
    pub button: u32, // 1, 2, 3
    pub function: String,
    pub argument: ArgumentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    pub class: String,
    pub instance: String,
    pub name: String,
    pub tags_mask: usize,
    pub is_floating: bool,
    pub monitor: i32,
}

pub struct Config {
    inner: TomlConfig,
}

impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: TomlConfig = toml::from_str(&content)?;
        Ok(Self { inner: config })
    }

    pub fn load_default() -> Self {
        // 如果配置文件不存在，使用默认配置
        let default_config_path = dirs::config_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap())
            .join("dwm")
            .join("config.toml");

        Self::load_from_file(&default_config_path).unwrap_or_else(|_| Self::default())
    }

    // 访问器方法
    pub fn border_px(&self) -> u32 {
        self.inner.appearance.border_px
    }

    pub fn snap(&self) -> u32 {
        self.inner.appearance.snap
    }

    pub fn dmenu_font(&self) -> &str {
        &self.inner.appearance.dmenu_font
    }

    pub fn status_bar_name(&self) -> &str {
        &self.inner.status_bar.name
    }

    pub fn m_fact(&self) -> f32 {
        self.inner.layout.m_fact
    }

    pub fn n_master(&self) -> u32 {
        self.inner.layout.n_master
    }

    pub fn tags_length(&self) -> usize {
        self.inner.layout.tags_length
    }

    pub fn tagmask(&self) -> u32 {
        (1 << self.tags_length()) - 1
    }

    // 转换方法
    pub fn get_keys(&self) -> Vec<Key> {
        let mut keys = Vec::new();

        for key_config in &self.inner.keybindings.keys {
            if let Some(key) = self.convert_key_config(key_config) {
                keys.push(key);
            }
        }

        // 添加标签键
        for i in 0..self.tags_length() {
            keys.extend(self.generate_tag_keys(i));
        }

        keys
    }

    pub fn get_buttons(&self) -> Vec<Button> {
        self.inner
            .mouse_bindings
            .buttons
            .iter()
            .filter_map(|btn| self.convert_button_config(btn))
            .collect()
    }

    pub fn get_rules(&self) -> Vec<Rule> {
        self.inner
            .rules
            .iter()
            .map(|rule| {
                Rule::new(
                    &rule.class,
                    &rule.instance,
                    &rule.name,
                    rule.tags_mask,
                    rule.is_floating,
                    rule.monitor,
                )
            })
            .collect()
    }

    pub fn get_dmenucmd(&self) -> Vec<String> {
        // 从配置中查找 dmenu 命令，或使用默认值
        self.inner
            .keybindings
            .keys
            .iter()
            .find(|k| k.function == "spawn" && k.key == "e")
            .and_then(|k| match &k.argument {
                ArgumentConfig::StringVec(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .unwrap_or_else(|| {
                vec![
                    "dmenu_run".to_string(),
                    "-m".to_string(),
                    "0".to_string(),
                    // 其他默认参数...
                ]
            })
    }

    pub fn get_termcmd(&self) -> Vec<String> {
        // 类似地从配置中获取终端命令
        ADVANCED_TERMINAL_PROBER
            .get_available_terminal()
            .map(|config| vec![config.command.clone()])
            .unwrap_or_else(|| vec!["x-terminal-emulator".to_string()])
    }

    // 私有辅助方法
    fn convert_key_config(&self, key_config: &KeyConfig) -> Option<Key> {
        let modifiers = self.parse_modifiers(&key_config.modifier);
        let keysym = self.parse_keysym(&key_config.key)?;
        let function = self.parse_function(&key_config.function)?;
        let arg = self.convert_argument(&key_config.argument);

        Some(Key::new(modifiers, keysym, Some(function), arg))
    }

    fn parse_modifiers(&self, modifiers: &[String]) -> u32 {
        let mut mask = 0;
        for modifier in modifiers {
            mask |= match modifier.as_str() {
                "Mod1" => Mod1Mask,
                "Mod4" => Mod4Mask,
                "Control" => ControlMask,
                "Shift" => ShiftMask,
                _ => 0,
            };
        }
        mask
    }

    fn parse_keysym(&self, key: &str) -> Option<u64> {
        match key {
            "Return" => Some(XK_Return),
            "Tab" => Some(XK_Tab),
            "space" => Some(XK_space),
            "j" => Some(XK_j),
            "k" => Some(XK_k),
            // 添加更多键映射...
            _ => None,
        }
    }

    fn parse_function(&self, func_name: &str) -> Option<fn(&dwm::Arg)> {
        match func_name {
            "spawn" => Some(Dwm::spawn),
            "focusstack" => Some(Dwm::focusstack),
            "quit" => Some(Dwm::quit),
            // 添加更多函数映射...
            _ => None,
        }
    }

    fn convert_argument(&self, arg: &ArgumentConfig) -> dwm::Arg {
        match arg {
            ArgumentConfig::Int(i) => dwm::Arg::I(*i),
            ArgumentConfig::UInt(u) => dwm::Arg::Ui(*u),
            ArgumentConfig::Float(f) => dwm::Arg::F(*f),
            ArgumentConfig::StringVec(v) => dwm::Arg::V(v.clone()),
            ArgumentConfig::String(s) => dwm::Arg::V(vec![s.clone()]),
        }
    }

    fn generate_tag_keys(&self, tag: usize) -> Vec<Key> {
        let key = match tag {
            0 => XK_1,
            1 => XK_2,
            2 => XK_3,
            3 => XK_4,
            4 => XK_5,
            5 => XK_6,
            6 => XK_7,
            7 => XK_8,
            8 => XK_9,
            _ => return vec![],
        };

        let modkey = self.parse_modifiers(&[self.inner.keybindings.modkey.clone()]);

        vec![
            Key::new(modkey, key, Some(Dwm::view), dwm::Arg::Ui(1 << tag)),
            Key::new(
                modkey | ControlMask,
                key,
                Some(Dwm::toggleview),
                dwm::Arg::Ui(1 << tag),
            ),
            Key::new(
                modkey | ShiftMask,
                key,
                Some(Dwm::tag),
                dwm::Arg::Ui(1 << tag),
            ),
            Key::new(
                modkey | ControlMask | ShiftMask,
                key,
                Some(Dwm::toggletag),
                dwm::Arg::Ui(1 << tag),
            ),
        ]
    }
}

impl Default for Config {
    fn default() -> Self {
        // 返回硬编码的默认配置，与原来的 Config 保持一致
        Self {
            inner: TomlConfig {
                appearance: AppearanceConfig {
                    border_px: 3,
                    snap: 32,
                    dmenu_font: "SauceCodePro Nerd Font Regular 11".to_string(),
                    status_bar_pad: 5,
                    broken: "broken".to_string(),
                },
                behavior: BehaviorConfig {
                    focus_follows_new_window: false,
                    resize_hints: true,
                    lock_fullscreen: true,
                },
                // 其他默认值...
                status_bar: StatusBarConfig {
                    name: "egui_bar".to_string(),
                    bar_0: "egui_bar_0".to_string(),
                    bar_1: "egui_bar_1".to_string(),
                },
                colors: ColorsConfig {
                    dark_sea_green1: "#afffd7".to_string(),
                    // 其他颜色...
                    black: "#000000".to_string(),
                    white: "#ffffff".to_string(),
                    transparent: 0,
                    opaque: 255,
                },
                layout: LayoutConfig {
                    m_fact: 0.55,
                    n_master: 1,
                    tags_length: 9,
                },
                keybindings: KeyBindingsConfig {
                    modkey: "Mod1".to_string(),
                    keys: vec![], // 将在运行时填充
                },
                mouse_bindings: MouseBindingsConfig {
                    buttons: vec![], // 将在运行时填充
                },
                rules: vec![], // 将在运行时填充
            },
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::Io(err)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        ConfigError::Parse(err)
    }
}

// 全局配置实例
pub static CONFIG: Lazy<Config> = Lazy::new(|| Config::load_default());
