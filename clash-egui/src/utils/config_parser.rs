use crate::clash::config::ClashConfig;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub struct ConfigParser;

impl ConfigParser {
    pub fn load_config<P: AsRef<Path>>(path: P) -> Result<ClashConfig> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        // 尝试解析配置文件
        match serde_yaml::from_str::<ClashConfig>(&contents) {
            Ok(mut config) => {
                // 解析成功后，从 external_controller 中提取控制器信息
                let parts: Vec<&str> = config.external_controller.split(':').collect();
                if parts.len() == 2 {
                    if let Ok(port) = parts[1].parse::<u16>() {
                        config.controller.port = port;
                    }
                    // 如果有 secret，也可以设置
                    if let Some(secret) = &config.secret {
                        config.controller.secret = secret.clone();
                    }
                }
                Ok(config)
            }
            Err(e) => {
                // 提供更详细的错误信息
                Err(anyhow::anyhow!(
                    "Failed to parse config file: {}\nContent: {}",
                    e,
                    contents
                ))
            }
        }
    }

    pub fn save_config<P: AsRef<Path>>(config: &ClashConfig, path: P) -> Result<()> {
        let yaml = serde_yaml::to_string(config).context("Failed to serialize config")?;

        let mut file = File::create(path)?;
        file.write_all(yaml.as_bytes())?;

        Ok(())
    }

    pub fn import_from_url(url: &str) -> Result<ClashConfig> {
        let response = reqwest::blocking::get(url)?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download config: HTTP {}",
                response.status()
            ));
        }

        let text = response.text()?;
        let config = serde_yaml::from_str(&text).context("Failed to parse downloaded config")?;

        Ok(config)
    }
}
