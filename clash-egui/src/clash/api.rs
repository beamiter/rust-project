use anyhow::{Context, Result};
use log::info;
use reqwest::blocking::{Client, Response};
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;

#[derive(Debug, Deserialize)]
pub struct ClashStatus {
    pub port: u16,
    pub mode: String,
    #[serde(rename = "redir-port")]
    pub redir_port: u16,
    #[serde(rename = "socks-port")]
    pub socks_port: u16,
    #[serde(rename = "mixed-port")]
    pub mixed_port: u16,
    #[serde(rename = "allow-lan")]
    pub allow_lan: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub history: Vec<ProxyHistory>,
    pub all: Option<Vec<String>>,
    pub now: Option<String>,
    pub udp: Option<bool>,
    pub tfo: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProxyHistory {
    pub time: String,
    pub delay: u64,
}

#[derive(Debug, Deserialize)]
pub struct TrafficInfo {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Deserialize)]
pub struct LogEntry {
    #[serde(rename = "type")]
    pub log_type: String,
    pub payload: String,
    pub time: String,
}

pub struct ApiClient {
    client: Client,
    base_url: String,
    secret: String,
    app_state: Option<Arc<Mutex<crate::ui::proxies::Proxies>>>,
}

impl ApiClient {
    pub fn new(base_url: &str, secret: &str) -> Self {
        let mut headers = header::HeaderMap::new();
        if !secret.is_empty() {
            headers.insert(
                "Authorization",
                format!("Bearer {}", secret).parse().unwrap(),
            );
        }

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self {
            client,
            base_url: base_url.to_string(),
            secret: secret.to_string(),
            app_state: None,
        }
    }

    pub fn get_status(&self) -> Result<ClashStatus> {
        let url = format!("{}/configs", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to get status: HTTP {}",
                response.status()
            ));
        }

        let status = response.json::<ClashStatus>()?;
        Ok(status)
    }

    // src/clash/api.rs

    pub fn set_global_proxy(&self, proxy_name: &str) -> Result<()> {
        // 设置超时较短的客户端，避免长时间阻塞
        let client = reqwest::blocking::ClientBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()?;
        // 尝试通过 Clash API 设置全局代理
        // 注意：具体实现取决于 Clash 的 API

        // 方法 1: 如果 Clash 支持直接设置全局代理
        let url = format!("{}/proxies/GLOBAL", self.base_url);

        #[derive(Serialize)]
        struct SwitchRequest {
            name: String,
        }

        let request = SwitchRequest {
            name: proxy_name.to_string(),
        };

        let response = client.put(&url).json(&request).send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to set global proxy: HTTP {}",
                response.status()
            ));
        }

        Ok(())
    }

    pub fn get_proxies(&self) -> Result<Vec<ProxyInfo>> {
        let url = format!("{}/proxies", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to get proxies: HTTP {}",
                response.status()
            ));
        }

        #[derive(Deserialize)]
        struct ProxiesResponse {
            proxies: std::collections::HashMap<String, ProxyInfo>,
        }

        let proxies_response = response.json::<ProxiesResponse>()?;
        let proxies: Vec<ProxyInfo> = proxies_response.proxies.into_values().collect();

        Ok(proxies)
    }

    pub fn get_proxy_delay(&self, name: &str) -> Result<u64> {
        let url = format!("{}/proxies/{}/delay", self.base_url, name);
        let response = self
            .client
            .get(&url)
            .query(&[
                ("timeout", "5000"),
                ("url", "http://www.gstatic.com/generate_204"),
            ])
            .send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to get proxy delay: HTTP {}",
                response.status()
            ));
        }

        #[derive(Deserialize)]
        struct DelayResponse {
            delay: u64,
        }

        let delay_response = response.json::<DelayResponse>()?;
        Ok(delay_response.delay)
    }

    pub fn switch_proxy(&self, group: &str, proxy: &str) -> Result<()> {
        let url = format!("{}/proxies/{}", self.base_url, group);

        #[derive(Serialize)]
        struct SwitchRequest {
            name: String,
        }

        let request = SwitchRequest {
            name: proxy.to_string(),
        };

        let response = self.client.put(&url).json(&request).send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to switch proxy: HTTP {}",
                response.status()
            ));
        }

        Ok(())
    }
    fn test_connection(&self) -> Result<()> {
        println!("开始测试连接...");
        let resp = reqwest::blocking::get("http://127.0.0.1:9090/traffic")?;
        println!("状态码: {}", resp.status());
        let text = resp.text()?;
        println!("响应内容: {}", text);
        Ok(())
    }
    pub fn get_traffic(&self) -> Result<TrafficInfo> {
        let url = format!("{}/traffic", self.base_url);
        info!("url: {}", url);

        // 使用curl命令，限制传输最长时间
        let output = std::process::Command::new("curl")
            .args(&[
                "-s", // 静默模式
                "-N", // 禁用缓冲，立即输出
                "-m", "5", // 总超时1秒
                &url,
            ])
            .output()?;

        if output.stdout.is_empty() {
            return Err(anyhow::anyhow!("未收到任何数据"));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        println!("curl获取到数据: {}", text);

        // 处理获取到的数据行
        for line in text.lines() {
            if !line.trim().is_empty() {
                match serde_json::from_str::<TrafficInfo>(line) {
                    Ok(traffic) => return Ok(traffic),
                    Err(_) => continue,
                }
            }
        }

        Err(anyhow::anyhow!("未找到有效的流量数据"))
    }

    pub fn get_logs(&self) -> Result<Vec<LogEntry>> {
        let url = format!("{}/logs", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to get logs: HTTP {}",
                response.status()
            ));
        }

        let logs = response.json::<Vec<LogEntry>>()?;
        Ok(logs)
    }

    pub fn set_app_state(&mut self, state: Arc<Mutex<crate::ui::proxies::Proxies>>) {
        self.app_state = Some(state);
    }

    pub fn get_app_state_mut(&self) -> Option<std::sync::MutexGuard<crate::ui::proxies::Proxies>> {
        if let Some(state) = &self.app_state {
            state.lock().ok()
        } else {
            None
        }
    }
}
