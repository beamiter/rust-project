use crate::clash::config::ClashConfig;
use crate::utils::config_parser::ConfigParser;
use anyhow::{Context, Result};
use log::{error, info};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

pub struct ClashCore {
    config: ClashConfig,
    config_path: PathBuf,
    process: Option<Child>,
    api_client: Arc<Mutex<crate::clash::api::ApiClient>>,
}

impl ClashCore {
    pub fn new() -> Self {
        // 获取默认配置路径
        let config_dir = directories::ProjectDirs::from("com", "clash-gui", "clash-egui")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("./config"));
        // println!("{:?}", config_dir);

        std::fs::create_dir_all(&config_dir).unwrap_or_else(|e| {
            error!("Failed to create config directory: {}", e);
        });

        let config_path = config_dir.join("config.yaml");

        // 加载或创建默认配置
        let config = if config_path.exists() {
            ConfigParser::load_config(&config_path).unwrap_or_else(|_| ClashConfig::default())
        } else {
            let default_config = ClashConfig::default();
            if let Err(e) = ConfigParser::save_config(&default_config, &config_path) {
                error!("Failed to save default config: {}", e);
            }
            default_config
        };

        let api_client = Arc::new(Mutex::new(crate::clash::api::ApiClient::new(
            &format!("http://127.0.0.1:{}", config.controller.port),
            &config.controller.secret,
        )));
        // println!("{:?}", config);

        Self {
            config,
            config_path,
            process: None,
            api_client,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        if self.is_running() {
            info!("Clash is already running");
            return Ok(());
        }

        // 确保配置已保存
        ConfigParser::save_config(&self.config, &self.config_path)?;

        // 启动 Clash 进程
        // 注意：这里假设 clash 命令可在 PATH 中找到
        // 实际应用中，可能需要捆绑 clash 二进制文件或使用 FFI
        let process = Command::new("clash")
            .arg("-f")
            .arg(&self.config_path)
            .spawn()
            .context("Failed to start clash process")?;

        self.process = Some(process);
        info!("Clash started with config: {:?}", self.config_path);

        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            process.kill().context("Failed to kill clash process")?;
            info!("Clash stopped");
        }

        Ok(())
    }

    // pub fn is_running(&self) -> bool {
    //     if let Some(child) = &self.process {
    //         match child.try_wait() {
    //             Ok(None) => true,
    //             _ => false,
    //         }
    //     } else {
    //         false
    //     }
    // }
    pub fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.process {
            match child.try_wait() {
                Ok(None) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn reload_config(&mut self) -> Result<()> {
        let was_running = self.is_running();

        if was_running {
            self.stop()?;
        }

        self.config = ConfigParser::load_config(&self.config_path)?;

        if was_running {
            self.start()?;
        }

        Ok(())
    }

    pub fn get_config(&self) -> &ClashConfig {
        &self.config
    }

    pub fn update_config(&mut self, config: ClashConfig) -> Result<()> {
        self.config = config;
        ConfigParser::save_config(&self.config, &self.config_path)?;

        Ok(())
    }

    pub fn get_api_client(&self) -> Arc<Mutex<crate::clash::api::ApiClient>> {
        Arc::clone(&self.api_client)
    }
}
