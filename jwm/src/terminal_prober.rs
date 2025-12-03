use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::process::Command;
use std::sync::RwLock;

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

        let priority_order = vec![
            "warp-terminal".to_string(),
            "terminator".to_string(),
            "gnome-terminal".to_string(),
            "jterm4".to_string(),
        ];

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
