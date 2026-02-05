use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;
use std::env;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TerminalConfig {
    pub command: String,
    pub execute_flag: String,
    pub title_flag: Option<String>,
    pub geometry_flag: Option<String>,
    pub working_dir_flag: Option<String>,
}

pub struct AdvancedTerminalProber {
    configs: HashMap<String, TerminalConfig>,
    priority_order: Vec<String>,
    cache: RwLock<HashMap<String, bool>>,
}

impl AdvancedTerminalProber {
    fn new() -> Self {
        let mut configs = HashMap::new();

        // Wayland-first terminals (often needed for DRM/udev compositors)
        configs.insert(
            "foot".to_string(),
            TerminalConfig {
                command: "foot".to_string(),
                execute_flag: "-e".to_string(),
                title_flag: Some("-T".to_string()),
                geometry_flag: None,
                working_dir_flag: Some("-D".to_string()),
            },
        );

        configs.insert(
            "wezterm".to_string(),
            TerminalConfig {
                command: "wezterm".to_string(),
                execute_flag: "start".to_string(),
                title_flag: Some("--class".to_string()),
                geometry_flag: None,
                working_dir_flag: Some("--cwd".to_string()),
            },
        );

        configs.insert(
            "alacritty".to_string(),
            TerminalConfig {
                command: "alacritty".to_string(),
                execute_flag: "-e".to_string(),
                title_flag: Some("--title".to_string()),
                geometry_flag: None,
                working_dir_flag: Some("--working-directory".to_string()),
            },
        );

        configs.insert(
            "kitty".to_string(),
            TerminalConfig {
                command: "kitty".to_string(),
                execute_flag: "--".to_string(),
                title_flag: Some("--title".to_string()),
                geometry_flag: Some("--geometry".to_string()),
                working_dir_flag: Some("--directory".to_string()),
            },
        );

        configs.insert(
            "weston-terminal".to_string(),
            TerminalConfig {
                command: "weston-terminal".to_string(),
                execute_flag: "--".to_string(),
                title_flag: None,
                geometry_flag: None,
                working_dir_flag: None,
            },
        );

        // Warp Terminal
        configs.insert(
            "warp-terminal".to_string(),
            TerminalConfig {
                command: "warp-terminal".to_string(),
                execute_flag: "-e".to_string(),
                title_flag: Some("--title".to_string()),
                geometry_flag: None,
                working_dir_flag: Some("--working-directory".to_string()),
            },
        );

        // Terminator
        configs.insert(
            "terminator".to_string(),
            TerminalConfig {
                command: "terminator".to_string(),
                execute_flag: "-e".to_string(),
                title_flag: Some("-T".to_string()),
                geometry_flag: Some("-g".to_string()),
                working_dir_flag: Some("--working-directory".to_string()),
            },
        );

        // GNOME Terminal
        configs.insert(
            "gnome-terminal".to_string(),
            TerminalConfig {
                command: "gnome-terminal".to_string(),
                execute_flag: "--".to_string(),
                title_flag: Some("--title".to_string()),
                geometry_flag: Some("--geometry".to_string()),
                working_dir_flag: Some("--working-directory".to_string()),
            },
        );

        // JTerm4
        configs.insert(
            "jterm4".to_string(),
            TerminalConfig {
                command: "jterm4".to_string(),
                execute_flag: "-e".to_string(),
                title_flag: Some("--title".to_string()),
                geometry_flag: None,
                working_dir_flag: Some("--workdir".to_string()),
            },
        );

        // Choose priority based on session hints.
        // - In udev/DRM (Wayland compositor) sessions, X11 terminals often won't show.
        // - In X11 sessions, Warp/Terminator/Gnome-terminal are usually fine.
        let is_wayland = env::var("WAYLAND_DISPLAY").is_ok()
            || env::var("XDG_SESSION_TYPE")
                .map(|v| v.eq_ignore_ascii_case("wayland"))
                .unwrap_or(false);
        let has_display = env::var("DISPLAY").is_ok();

        let priority_order = if is_wayland && !has_display {
            vec![
                "jterm4".to_string(),
                "foot".to_string(),
                "wezterm".to_string(),
                "alacritty".to_string(),
                "kitty".to_string(),
                "weston-terminal".to_string(),
                // Keep Warp last: it may depend on X11/desktop services.
                "warp-terminal".to_string(),
                "terminator".to_string(),
                "gnome-terminal".to_string(),
            ]
        } else {
            vec![
                "jterm4".to_string(),
                "warp-terminal".to_string(),
                "terminator".to_string(),
                "gnome-terminal".to_string(),
                "alacritty".to_string(),
                "kitty".to_string(),
                "wezterm".to_string(),
                "foot".to_string(),
                "weston-terminal".to_string(),
            ]
        };

        Self {
            configs,
            priority_order,
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_available_terminal(&self) -> Option<&TerminalConfig> {
        for terminal_name in &self.priority_order {
            if let Some(config) = self.configs.get(terminal_name) {
                if self.is_command_available(&config.command) {
                    println!("[get_available_terminal] {:?}", config);
                    return Some(config);
                }
            }
        }
        None
    }

    fn is_command_available(&self, cmd: &str) -> bool {
        {
            let cache_reader = self.cache.read().unwrap();
            if let Some(&cached_result) = cache_reader.get(cmd) {
                return cached_result;
            }
        }
        let result = self.check_command_exists(cmd);
        {
            let mut cache_writer = self.cache.write().unwrap();
            cache_writer.insert(cmd.to_string(), result);
        }
        result
    }

    fn check_command_exists(&self, cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn build_command(
        &self,
        command: &str,
        title: Option<&str>,
        working_dir: Option<&str>,
    ) -> Option<Vec<String>> {
        let config = self.get_available_terminal()?;
        let mut cmd = vec![config.command.clone()];
        if let (Some(title), Some(title_flag)) = (title, &config.title_flag) {
            cmd.push(title_flag.clone());
            cmd.push(title.to_string());
        }
        if let (Some(working_dir), Some(dir_flag)) = (working_dir, &config.working_dir_flag) {
            cmd.push(dir_flag.clone());
            cmd.push(working_dir.to_string());
        }
        cmd.push(config.execute_flag.clone());
        cmd.push(command.to_string());
        Some(cmd)
    }

    #[allow(dead_code)]
    pub fn clear_cache(&self) {
        let mut cache_writer = self.cache.write().unwrap();
        cache_writer.clear();
    }
}

pub static ADVANCED_TERMINAL_PROBER: Lazy<AdvancedTerminalProber> =
    Lazy::new(|| AdvancedTerminalProber::new());
