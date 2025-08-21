// config.rs
use cfg_if::cfg_if;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use x11::keysym::*;
use x11rb::protocol::xproto::ButtonIndex;
use x11rb::protocol::xproto::KeyButMask;

use std::fmt;
use std::rc::Rc;

use x11::keysym::{
    XK_Page_Down, XK_Page_Up, XK_Return, XK_Tab, XK_b, XK_c, XK_comma, XK_d, XK_e, XK_f, XK_h,
    XK_i, XK_j, XK_k, XK_l, XK_m, XK_o, XK_period, XK_q, XK_r, XK_space, XK_t, XK_0, XK_1, XK_2,
    XK_3, XK_4, XK_5, XK_6, XK_7, XK_8, XK_9,
};

use crate::jwm::WMFuncType;
use crate::jwm::{self, Jwm, LayoutEnum, WMRule, WMButton, WMKey, CLICK};
use crate::terminal_prober::ADVANCED_TERMINAL_PROBER;

macro_rules! status_bar_config {
    ($($feature:literal => $name:literal),* $(,)?) => {
        cfg_if! {
            $(
                if #[cfg(feature = $feature)] {
                    pub const STATUS_BAR_NAME: &str = $name;
                    pub const STATUS_BAR_0: &str = concat!($name, "_0");
                    pub const STATUS_BAR_1: &str = concat!($name, "_1");
                } else
            )*
            {
                pub const STATUS_BAR_NAME: &str = "egui_bar";
                pub const STATUS_BAR_0: &str = "egui_bar_0";
                pub const STATUS_BAR_1: &str = "egui_bar_1";
            }
        }
    };
}
status_bar_config!(
    "dioxus_bar" => "dioxus_bar",
    "egui_bar" => "egui_bar",
    "iced_bar" => "iced_bar",
    "gtk_bar" => "gtk_bar",
    "relm_bar" => "relm_bar",
    "tauri_bar" => "tauri_bar",
);

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorConfig {
    pub focus_follows_new_window: bool,
    pub resize_hints: bool,
    pub lock_fullscreen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusBarConfig {
    pub base_name: String,
}

impl StatusBarConfig {
    /// 获取带索引的状态栏名称
    pub fn get_instance_name(&self, index: usize) -> String {
        format!("{}_{}", self.base_name, index)
    }
    /// 获取状态栏 0 的名称
    pub fn get_bar_instance_0(&self) -> String {
        self.get_instance_name(0)
    }
    /// 获取状态栏 1 的名称
    pub fn get_bar_instance_1(&self) -> String {
        self.get_instance_name(1)
    }
    /// 获取基础名称
    pub fn get_base_name(&self) -> &str {
        &self.base_name
    }
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

impl Default for Config {
    fn default() -> Self {
        Self {
            inner: TomlConfig {
                appearance: AppearanceConfig {
                    border_px: 3,
                    snap: 32,
                    dmenu_font: "SauceCodePro Nerd Font Regular 11".to_string(),
                    status_bar_pad: 5,
                },
                behavior: BehaviorConfig {
                    focus_follows_new_window: false,
                    resize_hints: true,
                    lock_fullscreen: true,
                },
                status_bar: StatusBarConfig {
                    base_name: STATUS_BAR_NAME.to_string(),
                },
                colors: ColorsConfig {
                    dark_sea_green1: "#afffd7".to_string(),
                    dark_sea_green2: "#afffaf".to_string(),
                    pale_turquoise1: "#afffff".to_string(),
                    light_sky_blue1: "#afd7ff".to_string(),
                    grey84: "#d7d7d7".to_string(),
                    cyan: "#00ffd7".to_string(),
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
                    keys: Self::get_default_keys(),
                },
                mouse_bindings: MouseBindingsConfig {
                    buttons: Self::get_default_button_configs(),
                },
                rules: Self::get_default_rules(),
            },
        }
    }
}

#[allow(dead_code)]
impl Config {
    // 获取默认按键绑定
    fn get_default_keys() -> Vec<KeyConfig> {
        vec![
            // 应用启动
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "e".to_string(),
                function: "spawn".to_string(),
                argument: ArgumentConfig::StringVec(vec![
                    "dmenu_run".to_string(),
                    "-m".to_string(),
                    "0".to_string(),
                    "-fn".to_string(),
                    "SauceCodePro Nerd Font Regular 11".to_string(),
                    "-nb".to_string(),
                    "#afd7ff".to_string(),
                    "-nf".to_string(),
                    "#afffff".to_string(),
                    "-sb".to_string(),
                    "#000000".to_string(),
                    "-sf".to_string(),
                    "#d7d7d7".to_string(),
                    "-b".to_string(),
                ]),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "r".to_string(),
                function: "spawn".to_string(),
                argument: ArgumentConfig::StringVec(vec![
                    "dmenu_run".to_string(),
                    "-m".to_string(),
                    "0".to_string(),
                    "-fn".to_string(),
                    "SauceCodePro Nerd Font Regular 11".to_string(),
                    "-nb".to_string(),
                    "#afd7ff".to_string(),
                    "-nf".to_string(),
                    "#afffff".to_string(),
                    "-sb".to_string(),
                    "#000000".to_string(),
                    "-sf".to_string(),
                    "#d7d7d7".to_string(),
                    "-b".to_string(),
                ]),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "Return".to_string(),
                function: "spawn".to_string(),
                argument: ArgumentConfig::StringVec(Self::get_termcmd()),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "b".to_string(),
                function: "togglebar".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            // 窗口焦点控制
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "j".to_string(),
                function: "focusstack".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "k".to_string(),
                function: "focusstack".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            // 主窗口数量控制
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "i".to_string(),
                function: "incnmaster".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "d".to_string(),
                function: "incnmaster".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            // 窗口大小调整
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "h".to_string(),
                function: "setmfact".to_string(),
                argument: ArgumentConfig::Float(-0.025),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "l".to_string(),
                function: "setmfact".to_string(),
                argument: ArgumentConfig::Float(0.025),
            },
            // 客户端高度调整
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "h".to_string(),
                function: "setcfact".to_string(),
                argument: ArgumentConfig::Float(0.2),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "l".to_string(),
                function: "setcfact".to_string(),
                argument: ArgumentConfig::Float(-0.2),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "o".to_string(),
                function: "setcfact".to_string(),
                argument: ArgumentConfig::Float(0.0),
            },
            // 窗口移动
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "j".to_string(),
                function: "movestack".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "k".to_string(),
                function: "movestack".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            // 主窗口切换
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "Return".to_string(),
                function: "zoom".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            // 标签切换
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "Tab".to_string(),
                function: "loopview".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "Tab".to_string(),
                function: "loopview".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "Page_Up".to_string(),
                function: "loopview".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "Page_Down".to_string(),
                function: "loopview".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            // 窗口关闭
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "c".to_string(),
                function: "killclient".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            // 布局切换
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "t".to_string(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::String("tile".to_string()),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "f".to_string(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::String("float".to_string()),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "m".to_string(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::String("monocle".to_string()),
            },
            // 布局切换
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "space".to_string(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "space".to_string(),
                function: "togglefloating".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "f".to_string(),
                function: "togglefullscr".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            // 全标签视图
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "0".to_string(),
                function: "view".to_string(),
                argument: ArgumentConfig::UInt(!0), // 所有标签
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "0".to_string(),
                function: "tag".to_string(),
                argument: ArgumentConfig::UInt(!0), // 所有标签
            },
            // 显示器切换
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "comma".to_string(),
                function: "focusmon".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string()],
                key: "period".to_string(),
                function: "focusmon".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "comma".to_string(),
                function: "tagmon".to_string(),
                argument: ArgumentConfig::Int(-1),
            },
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "period".to_string(),
                function: "tagmon".to_string(),
                argument: ArgumentConfig::Int(1),
            },
            // 退出
            KeyConfig {
                modifier: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "q".to_string(),
                function: "quit".to_string(),
                argument: ArgumentConfig::Int(0),
            },
        ]
    }

    // 获取默认鼠标绑定配置
    fn get_default_button_configs() -> Vec<ButtonConfig> {
        vec![
            ButtonConfig {
                click_type: "ClkLtSymbol".to_string(),
                modifier: vec![],
                button: ButtonIndex::M1.into(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkLtSymbol".to_string(),
                modifier: vec![],
                button: ButtonIndex::M3.into(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::String("monocle".to_string()),
            },
            ButtonConfig {
                click_type: "ClkWinTitle".to_string(),
                modifier: vec![],
                button: ButtonIndex::M2.into(),
                function: "zoom".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkStatusText".to_string(),
                modifier: vec![],
                button: ButtonIndex::M2.into(),
                function: "spawn".to_string(),
                argument: ArgumentConfig::StringVec(Self::get_termcmd()),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec!["Mod1".to_string()],
                button: ButtonIndex::M1.into(),
                function: "movemouse".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec!["Mod1".to_string()],
                button: ButtonIndex::M2.into(),
                function: "togglefloating".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec!["Mod1".to_string()],
                button: ButtonIndex::M3.into(),
                function: "resizemouse".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![],
                button: ButtonIndex::M1.into(),
                function: "view".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![],
                button: ButtonIndex::M3.into(),
                function: "toggleview".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec!["Mod1".to_string()],
                button: ButtonIndex::M1.into(),
                function: "tag".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec!["Mod1".to_string()],
                button: ButtonIndex::M3.into(),
                function: "toggletag".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
        ]
    }

    // 获取默认规则
    fn get_default_rules() -> Vec<RuleConfig> {
        vec![RuleConfig {
            class: "broken".to_string(),
            instance: "broken".to_string(),
            name: "broken".to_string(),
            tags_mask: 0,
            is_floating: true,
            monitor: -1,
        }]
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: TomlConfig = toml::from_str(&content)?;
        Ok(Self { inner: config })
    }

    pub fn load_default() -> Self {
        // 如果配置文件不存在，使用默认配置
        let default_config_path = dirs::config_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap())
            .join("jwm")
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

    pub fn status_bar_pad(&self) -> i32 {
        self.inner.appearance.status_bar_pad
    }

    pub fn dmenu_font(&self) -> &str {
        &self.inner.appearance.dmenu_font
    }

    pub fn status_bar_base_name(&self) -> &str {
        &self.inner.status_bar.base_name
    }

    pub fn status_bar_instance_0(&self) -> String {
        self.inner.status_bar.get_bar_instance_0()
    }

    pub fn status_bar_instance_1(&self) -> String {
        self.inner.status_bar.get_bar_instance_1()
    }

    pub fn status_bar_config(&self) -> &StatusBarConfig {
        &self.inner.status_bar
    }

    pub fn get_status_bar_instance_name(&self, index: usize) -> String {
        self.inner.status_bar.get_instance_name(index)
    }

    pub fn colors(&self) -> &ColorsConfig {
        &self.inner.colors
    }

    pub fn behavior(&self) -> &BehaviorConfig {
        &self.inner.behavior
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
    pub fn get_keys(&self) -> Vec<WMKey> {
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

    pub fn get_rules(&self) -> Vec<WMRule> {
        self.inner
            .rules
            .iter()
            .map(|rule| {
                WMRule::new(
                    rule.class.clone(),
                    rule.instance.clone(),
                    rule.name.clone(),
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

    pub fn get_termcmd() -> Vec<String> {
        // 类似地从配置中获取终端命令
        ADVANCED_TERMINAL_PROBER
            .get_available_terminal()
            .map(|config| vec![config.command.clone()])
            .unwrap_or_else(|| {
                println!("terminator fallback");
                vec!["x-terminal-emulator".to_string()]
            })
    }

    fn convert_button_config(&self, btn_config: &ButtonConfig) -> Option<WMButton> {
        let click_type = self.parse_click_type(&btn_config.click_type)?;
        let modifiers = self.parse_modifiers(&btn_config.modifier);
        let button = ButtonIndex::from(btn_config.button as u8);
        let function = self.parse_function(&btn_config.function)?;
        let arg = self.convert_argument(&btn_config.argument);

        Some(WMButton::new(
            click_type,
            modifiers,
            button,
            Some(function),
            arg,
        ))
    }

    fn parse_click_type(&self, click_type: &str) -> Option<u32> {
        match click_type {
            "ClkLtSymbol" => Some(CLICK::ClkLtSymbol as u32),
            "ClkWinTitle" => Some(CLICK::ClkWinTitle as u32),
            "ClkStatusText" => Some(CLICK::ClkStatusText as u32),
            "ClkClientWin" => Some(CLICK::ClkClientWin as u32),
            "ClkTagBar" => Some(CLICK::ClkTagBar as u32),
            "ClkRootWin" => Some(CLICK::ClkRootWin as u32),
            _ => {
                eprintln!("Unknown click type: {}", click_type);
                None
            }
        }
    }

    // 扩展 parse_function 以支持更多函数
    fn parse_function(&self, func_name: &str) -> Option<WMFuncType> {
        match func_name {
            // 窗口管理
            "spawn" => Some(Jwm::spawn),
            "focusstack" => Some(Jwm::focusstack),
            "focusmon" => Some(Jwm::focusmon),
            "quit" => Some(Jwm::quit),
            "killclient" => Some(Jwm::killclient),
            "zoom" => Some(Jwm::zoom),

            // 布局相关
            "setlayout" => Some(Jwm::setlayout),
            "togglefloating" => Some(Jwm::togglefloating),
            "togglefullscr" => Some(Jwm::togglefullscr),
            "togglebar" => Some(Jwm::togglebar),
            "setmfact" => Some(Jwm::setmfact),
            "setcfact" => Some(Jwm::setcfact),
            "incnmaster" => Some(Jwm::incnmaster),
            "movestack" => Some(Jwm::movestack),

            // 标签相关
            "view" => Some(Jwm::view),
            "tag" => Some(Jwm::tag),
            "toggleview" => Some(Jwm::toggleview),
            "toggletag" => Some(Jwm::toggletag),
            "tagmon" => Some(Jwm::tagmon),
            "loopview" => Some(Jwm::loopview),

            // 鼠标相关
            "movemouse" => Some(Jwm::movemouse),
            "resizemouse" => Some(Jwm::resizemouse),

            _ => {
                eprintln!("Unknown function: {}", func_name);
                None
            }
        }
    }

    // 扩展 parse_keysym 以支持更多按键
    fn parse_keysym(&self, key: &str) -> Option<u64> {
        match key {
            // 特殊键
            "Return" => Some(XK_Return.into()),
            "Tab" => Some(XK_Tab.into()),
            "space" => Some(XK_space.into()),
            "Page_Up" => Some(XK_Page_Up.into()),
            "Page_Down" => Some(XK_Page_Down.into()),
            "comma" => Some(XK_comma.into()),
            "period" => Some(XK_period.into()),

            // 字母键
            "a" => Some(XK_a.into()),
            "b" => Some(XK_b.into()),
            "c" => Some(XK_c.into()),
            "d" => Some(XK_d.into()),
            "e" => Some(XK_e.into()),
            "f" => Some(XK_f.into()),
            "g" => Some(XK_g.into()),
            "h" => Some(XK_h.into()),
            "i" => Some(XK_i.into()),
            "j" => Some(XK_j.into()),
            "k" => Some(XK_k.into()),
            "l" => Some(XK_l.into()),
            "m" => Some(XK_m.into()),
            "n" => Some(XK_n.into()),
            "o" => Some(XK_o.into()),
            "p" => Some(XK_p.into()),
            "q" => Some(XK_q.into()),
            "r" => Some(XK_r.into()),
            "s" => Some(XK_s.into()),
            "t" => Some(XK_t.into()),
            "u" => Some(XK_u.into()),
            "v" => Some(XK_v.into()),
            "w" => Some(XK_w.into()),
            "x" => Some(XK_x.into()),
            "y" => Some(XK_y.into()),
            "z" => Some(XK_z.into()),

            // 数字键
            "0" => Some(XK_0.into()),
            "1" => Some(XK_1.into()),
            "2" => Some(XK_2.into()),
            "3" => Some(XK_3.into()),
            "4" => Some(XK_4.into()),
            "5" => Some(XK_5.into()),
            "6" => Some(XK_6.into()),
            "7" => Some(XK_7.into()),
            "8" => Some(XK_8.into()),
            "9" => Some(XK_9.into()),

            // 功能键
            "F1" => Some(XK_F1.into()),
            "F2" => Some(XK_F2.into()),
            "F3" => Some(XK_F3.into()),
            "F4" => Some(XK_F4.into()),
            "F5" => Some(XK_F5.into()),
            "F6" => Some(XK_F6.into()),
            "F7" => Some(XK_F7.into()),
            "F8" => Some(XK_F8.into()),
            "F9" => Some(XK_F9.into()),
            "F10" => Some(XK_F10.into()),
            "F11" => Some(XK_F11.into()),
            "F12" => Some(XK_F12.into()),

            // 方向键
            "Left" => Some(XK_Left.into()),
            "Right" => Some(XK_Right.into()),
            "Up" => Some(XK_Up.into()),
            "Down" => Some(XK_Down.into()),

            // 其他常用键
            "Escape" => Some(XK_Escape.into()),
            "BackSpace" => Some(XK_BackSpace.into()),
            "Delete" => Some(XK_Delete.into()),
            "Home" => Some(XK_Home.into()),
            "End" => Some(XK_End.into()),

            _ => {
                eprintln!("Unknown key: {}", key);
                None
            }
        }
    }

    // 扩展 parse_modifiers 以支持更多修饰键
    fn parse_modifiers(&self, modifiers: &[String]) -> KeyButMask {
        let mut mask = KeyButMask::default();
        for modifier in modifiers {
            mask |= match modifier.as_str() {
                "Mod1" | "Alt" => KeyButMask::MOD1,
                "Mod2" => KeyButMask::MOD2,
                "Mod3" => KeyButMask::MOD3,
                "Mod4" | "Super" | "Win" => KeyButMask::MOD4,
                "Mod5" => KeyButMask::MOD5,
                "Control" | "Ctrl" => KeyButMask::CONTROL,
                "Shift" => KeyButMask::SHIFT,
                "Lock" | "CapsLock" => KeyButMask::LOCK,
                _ => {
                    eprintln!("Unknown modifier: {}", modifier);
                    KeyButMask::default()
                }
            };
        }
        mask
    }

    // 扩展 convert_argument 以支持布局参数
    fn convert_argument(&self, arg: &ArgumentConfig) -> jwm::WMArgEnum {
        match arg {
            ArgumentConfig::Int(i) => jwm::WMArgEnum::Int(*i),
            ArgumentConfig::UInt(u) => jwm::WMArgEnum::UInt(*u),
            ArgumentConfig::Float(f) => jwm::WMArgEnum::Float(*f),
            ArgumentConfig::StringVec(v) => jwm::WMArgEnum::StringVec(v.clone()),
            ArgumentConfig::String(s) => {
                // 特殊处理布局字符串
                match s.as_str() {
                    "tile" => jwm::WMArgEnum::Layout(Rc::new(LayoutEnum::try_from(0).unwrap())),
                    "float" => jwm::WMArgEnum::Layout(Rc::new(LayoutEnum::try_from(1).unwrap())),
                    "monocle" => jwm::WMArgEnum::Layout(Rc::new(LayoutEnum::try_from(2).unwrap())),
                    _ => jwm::WMArgEnum::StringVec(vec![s.clone()]),
                }
            }
        }
    }

    // 生成默认鼠标绑定的辅助方法
    fn get_default_buttons(&self) -> Vec<ButtonConfig> {
        vec![
            ButtonConfig {
                click_type: "ClkLtSymbol".to_string(),
                modifier: vec![],
                button: ButtonIndex::M1.into(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkLtSymbol".to_string(),
                modifier: vec![],
                button: ButtonIndex::M3.into(),
                function: "setlayout".to_string(),
                argument: ArgumentConfig::String("monocle".to_string()),
            },
            ButtonConfig {
                click_type: "ClkWinTitle".to_string(),
                modifier: vec![],
                button: ButtonIndex::M2.into(),
                function: "zoom".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkStatusText".to_string(),
                modifier: vec![],
                button: ButtonIndex::M2.into(),
                function: "spawn".to_string(),
                argument: ArgumentConfig::StringVec(Self::get_termcmd()),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec![self.inner.keybindings.modkey.clone()],
                button: ButtonIndex::M1.into(),
                function: "movemouse".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec![self.inner.keybindings.modkey.clone()],
                button: ButtonIndex::M2.into(),
                function: "togglefloating".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkClientWin".to_string(),
                modifier: vec![self.inner.keybindings.modkey.clone()],
                button: ButtonIndex::M3.into(),
                function: "resizemouse".to_string(),
                argument: ArgumentConfig::Int(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![],
                button: ButtonIndex::M1.into(),
                function: "view".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![],
                button: ButtonIndex::M3.into(),
                function: "toggleview".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![self.inner.keybindings.modkey.clone()],
                button: ButtonIndex::M1.into(),
                function: "tag".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
            ButtonConfig {
                click_type: "ClkTagBar".to_string(),
                modifier: vec![self.inner.keybindings.modkey.clone()],
                button: ButtonIndex::M3.into(),
                function: "toggletag".to_string(),
                argument: ArgumentConfig::UInt(0),
            },
        ]
    }

    pub fn get_buttons(&self) -> Vec<WMButton> {
        let button_configs = if self.inner.mouse_bindings.buttons.is_empty() {
            self.get_default_buttons()
        } else {
            self.inner.mouse_bindings.buttons.clone()
        };

        button_configs
            .iter()
            .filter_map(|btn| self.convert_button_config(btn))
            .collect()
    }

    // 私有辅助方法
    fn convert_key_config(&self, key_config: &KeyConfig) -> Option<WMKey> {
        let modifiers = self.parse_modifiers(&key_config.modifier);
        let keysym = self.parse_keysym(&key_config.key)?;
        let function = self.parse_function(&key_config.function)?;
        let arg = self.convert_argument(&key_config.argument);

        Some(WMKey::new(modifiers, keysym as u32, Some(function), arg))
    }

    fn generate_tag_keys(&self, tag: usize) -> Vec<WMKey> {
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
            WMKey::new(
                modkey,
                key.into(),
                Some(Jwm::view),
                jwm::WMArgEnum::UInt(1 << tag),
            ),
            WMKey::new(
                modkey | KeyButMask::CONTROL,
                key.into(),
                Some(Jwm::toggleview),
                jwm::WMArgEnum::UInt(1 << tag),
            ),
            WMKey::new(
                modkey | KeyButMask::SHIFT,
                key.into(),
                Some(Jwm::tag),
                jwm::WMArgEnum::UInt(1 << tag),
            ),
            WMKey::new(
                modkey | KeyButMask::CONTROL | KeyButMask::SHIFT,
                key.into(),
                Some(Jwm::toggletag),
                jwm::WMArgEnum::UInt(1 << tag),
            ),
        ]
    }

    /// 保存当前配置到指定文件
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let toml_string =
            toml::to_string_pretty(&self.inner).map_err(|e| ConfigError::Serialize(e))?;

        // 确保目录存在
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, toml_string)?;
        Ok(())
    }

    /// 保存配置到默认位置
    pub fn save_default(&self) -> Result<(), ConfigError> {
        let config_path = Self::get_default_config_path();
        self.save_to_file(config_path)
    }

    /// 获取默认配置文件路径
    pub fn get_default_config_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| std::env::current_dir().unwrap())
            .join("jwm")
            .join("config.toml")
    }

    /// 生成配置文件模板并保存
    pub fn generate_template<P: AsRef<Path>>(path: P) -> Result<(), ConfigError> {
        let default_config = Self::default();
        default_config.save_to_file(path)
    }

    /// 备份当前配置文件
    pub fn backup_config<P: AsRef<Path>>(
        original_path: P,
    ) -> Result<std::path::PathBuf, ConfigError> {
        let original = original_path.as_ref();
        let backup_path = original.with_extension("toml.backup");

        if original.exists() {
            fs::copy(original, &backup_path)?;
        }

        Ok(backup_path)
    }

    /// 从备份恢复配置文件
    pub fn restore_from_backup<P: AsRef<Path>>(
        backup_path: P,
        target_path: P,
    ) -> Result<(), ConfigError> {
        let backup = backup_path.as_ref();
        let target = target_path.as_ref();

        if !backup.exists() {
            return Err(ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Backup file not found",
            )));
        }

        fs::copy(backup, target)?;
        Ok(())
    }

    /// 验证配置文件是否有效
    pub fn validate_config_file<P: AsRef<Path>>(path: P) -> Result<(), ConfigError> {
        let content = fs::read_to_string(path)?;
        let _config: TomlConfig = toml::from_str(&content)?;
        Ok(())
    }

    /// 合并配置（用于部分更新）
    pub fn merge_config(&mut self, other: TomlConfig) {
        // 这里可以实现选择性合并逻辑
        self.inner = other;
    }

    /// 重新加载配置文件
    pub fn reload(&mut self) -> Result<(), ConfigError> {
        let config_path = Self::get_default_config_path();
        if config_path.exists() {
            let new_config = Self::load_from_file(&config_path)?;
            self.inner = new_config.inner;
        }
        Ok(())
    }

    /// 检查配置文件是否存在
    pub fn config_exists() -> bool {
        Self::get_default_config_path().exists()
    }

    /// 获取配置文件的最后修改时间
    pub fn get_config_modified_time() -> Result<std::time::SystemTime, ConfigError> {
        let config_path = Self::get_default_config_path();
        let metadata = fs::metadata(config_path)?;
        Ok(metadata.modified()?)
    }
}

// 完整的 ConfigError 定义和 From trait 实现
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(err) => write!(f, "IO error: {}", err),
            ConfigError::Parse(err) => write!(f, "Parse error: {}", err),
            ConfigError::Serialize(err) => write!(f, "Serialize error: {}", err),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(err) => Some(err),
            ConfigError::Parse(err) => Some(err),
            ConfigError::Serialize(err) => Some(err),
        }
    }
}

// 添加缺失的 From trait 实现
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

impl From<toml::ser::Error> for ConfigError {
    fn from(err: toml::ser::Error) -> Self {
        ConfigError::Serialize(err)
    }
}

// 全局配置实例
pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    // 加载配置
    let config = Config::load_default();

    // 生成配置文件模板（如果不存在）
    if !Config::config_exists() {
        Config::generate_template(Config::get_default_config_path()).unwrap();
        println!(
            "Generated default config file at: {:?}",
            Config::get_default_config_path()
        );
    }

    // Usage test case:
    // // 备份现有配置
    // let backup_path = Config::backup_config(Config::get_default_config_path()).unwrap();
    // println!("Backup created at: {:?}", backup_path);
    //
    // // 保存当前配置
    // config.save_default().unwrap();
    // println!("Configuration saved successfully!");
    //
    // // 验证配置文件
    // Config::validate_config_file(Config::get_default_config_path()).unwrap();
    // println!("Configuration file is valid!");
    //
    // // 重新加载配置
    // config.reload().unwrap();
    println!("Configuration reloaded!");

    return config;
});
