use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime};
use console::style;
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Select};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tokio;

const CACHE_DIR: &str = ".bob_cache";
const TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct VersionInfo {
    version: String,
    url: String,
    published_at: Option<String>,
    description: Option<String>,
}

impl VersionInfo {
    fn get_date(&self) -> Option<NaiveDateTime> {
        let parsed_date = self.published_at.as_ref().and_then(|date_str| {
            DateTime::parse_from_rfc3339(date_str.as_str())
                .map(|dt| dt.naive_utc())
                .ok()
        });
        return parsed_date;
    }
}

impl Ord for VersionInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        // 首先按日期比较，如果日期不存在或相等，则按版本字符串比较
        match (self.get_date(), other.get_date()) {
            (Some(date1), Some(date2)) => date2.cmp(&date1), // 降序排列
            _ => other.version.cmp(&self.version),
        }
    }
}

impl PartialOrd for VersionInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for VersionInfo {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

impl Eq for VersionInfo {}

enum EditorType {
    Vim,
    Neovim,
}

impl EditorType {
    fn as_str(&self) -> &'static str {
        match self {
            EditorType::Vim => "vim",
            EditorType::Neovim => "neovim",
        }
    }

    fn api_url(&self) -> &'static str {
        match self {
            EditorType::Vim => "https://api.github.com/repos/vim/vim-appimage/releases",
            EditorType::Neovim => "https://api.github.com/repos/neovim/neovim/releases",
        }
    }

    fn asset_filter(&self, name: &str) -> bool {
        match self {
            EditorType::Vim => name.starts_with("Vim") && name.ends_with("AppImage"),
            EditorType::Neovim => name == "nvim.appimage" || name == "nvim-linux-x86_64.appimage",
        }
    }

    fn binary_name(&self) -> &'static str {
        match self {
            EditorType::Vim => "vim",
            EditorType::Neovim => "nvim",
        }
    }
}

struct BobManager {
    client: Client,
}

impl BobManager {
    fn new() -> Self {
        // 确保缓存目录存在
        fs::create_dir_all(CACHE_DIR).unwrap_or_else(|_| {
            eprintln!("无法创建缓存目录，继续但不会缓存结果");
        });

        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
                .user_agent("bob-editor-manager")
                .build()
                .expect("无法创建HTTP客户端"),
        }
    }

    async fn fetch_versions(&self, editor: &EditorType) -> Result<Vec<VersionInfo>> {
        let cache_file = format!("{}/{}_versions.json", CACHE_DIR, editor.as_str());

        // 尝试从缓存读取
        if let Ok(content) = fs::read_to_string(&cache_file) {
            if let Ok(versions) = serde_json::from_str::<Vec<VersionInfo>>(&content) {
                println!("使用缓存的版本信息");
                return Ok(versions);
            }
        }

        println!("正在从GitHub获取{}版本信息...", editor.as_str());
        let url = editor.api_url();

        let releases: serde_json::Value = self
            .client
            .get(url)
            .send()
            .await?
            .json()
            .await
            .context("解析GitHub API响应失败")?;

        let version_regex = Regex::new(r"releases/download/(.*?)/").unwrap();

        let mut versions = Vec::new();

        if let Some(releases_array) = releases.as_array() {
            for release in releases_array.iter().take(15) {
                // 只处理最新的15个版本
                if let Some(assets) = release["assets"].as_array() {
                    let published_at = release["published_at"].as_str().map(String::from);
                    let description = release["body"].as_str().map(String::from);

                    for asset in assets {
                        if let Some(name) = asset["name"].as_str() {
                            if editor.asset_filter(name) {
                                if let Some(url) = asset["browser_download_url"].as_str() {
                                    if let Some(caps) = version_regex.captures(url) {
                                        if let Some(version_match) = caps.get(1) {
                                            versions.push(VersionInfo {
                                                version: version_match.as_str().to_string(),
                                                url: url.to_string(),
                                                published_at: published_at.clone(),
                                                description: description.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 对版本进行排序
        versions.sort();

        // 缓存结果
        if let Ok(json) = serde_json::to_string(&versions) {
            let _ = fs::write(&cache_file, json); // 忽略错误，非关键功能
        }

        Ok(versions)
    }

    async fn download_file(&self, url: &str, file_path: &str) -> Result<()> {
        // 发送HEAD请求获取文件大小
        let response = self.client.head(url).send().await?;
        let total_size = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|ct_len| ct_len.to_str().ok())
            .and_then(|ct_len| ct_len.parse::<u64>().ok())
            .unwrap_or(0);

        // 创建带有样式的进度条
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .expect("无效的进度条模板")
                .progress_chars("#>-"),
        );

        // 下载文件
        let mut request = self.client.get(url).send().await?;
        let mut file = File::create(file_path)?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = request.chunk().await? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        pb.finish_with_message("下载完成");

        // 设置文件权限
        let mut perms = fs::metadata(file_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(file_path, perms)?;

        Ok(())
    }

    fn install_file(&self, file_path: &str, target_path: &str) -> Result<()> {
        print!("正在移动文件到 {}... ", target_path);
        io::stdout().flush()?;

        let status = Command::new("sudo")
            .args(["mv", file_path, target_path])
            .status()?;

        if !status.success() {
            println!("失败");
            return Err(anyhow::anyhow!("移动文件失败"));
        }
        println!("成功");

        print!("设置执行权限... ");
        io::stdout().flush()?;

        let status = Command::new("sudo")
            .args(["chmod", "755", target_path])
            .status()?;

        if !status.success() {
            println!("失败");
            return Err(anyhow::anyhow!("设置权限失败"));
        }
        println!("成功");

        Ok(())
    }

    async fn process_editor(&self, editor: &EditorType, version_info: &VersionInfo) -> Result<()> {
        let binary_name = editor.binary_name();
        let file_path = format!("/tmp/{}.appimage", binary_name);
        let target_path = format!("/usr/local/bin/{}", binary_name);

        println!("正在下载 {} v{}...", editor.as_str(), version_info.version);
        self.download_file(&version_info.url, &file_path).await?;

        println!("正在安装 {} v{}...", editor.as_str(), version_info.version);
        self.install_file(&file_path, &target_path)?;

        // 验证安装
        let version_check = Command::new(&target_path)
            .arg("--version")
            .output()
            .context("无法验证安装")?;

        if version_check.status.success() {
            println!(
                "{} 安装成功:\n{}",
                style(editor.as_str()).green().bold(),
                String::from_utf8_lossy(&version_check.stdout).trim()
            );
        }

        Ok(())
    }

    async fn clear_cache(&self) -> Result<()> {
        if Path::new(CACHE_DIR).exists() {
            println!("正在清除缓存...");
            fs::remove_dir_all(CACHE_DIR)?;
            fs::create_dir(CACHE_DIR)?;
            println!("缓存已清除");
        }
        Ok(())
    }
}

async fn display_version_menu(manager: &BobManager, editor: EditorType) -> Result<()> {
    let versions = manager.fetch_versions(&editor).await?;

    if versions.is_empty() {
        println!("没有找到可用版本");
        return Ok(());
    }
    for v in &versions {
        println!("{:?}", v);
    }

    // 构建版本选择菜单
    let version_labels: Vec<String> = versions
        .iter()
        .map(|v| {
            let date = v
                .get_date()
                .map(|d| format!("{}", d.format("%Y-%m-%d")))
                .unwrap_or_else(|| "未知日期".to_string());

            format!("{} ({})", v.version, date)
        })
        .collect();

    println!("请选择要安装的{}版本:", editor.as_str());

    // 创建模糊选择菜单
    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("选择版本 (ESC取消)")
        .default(0)
        .items(&version_labels)
        .interact_opt()?;

    match selection {
        Some(index) => {
            // 显示选择的版本详情
            let version = &versions[index];
            if let Some(desc) = &version.description {
                println!("\n版本详情:\n{}", style(desc).dim());
            }

            if dialoguer::Confirm::new()
                .with_prompt(format!(
                    "确定要安装 {} v{}?",
                    editor.as_str(),
                    version.version
                ))
                .default(true)
                .interact()?
            {
                manager.process_editor(&editor, version).await?;
            }
        }
        None => println!("已取消安装"),
    }

    Ok(())
}

async fn main_menu() -> Result<()> {
    let manager = BobManager::new();

    loop {
        println!("\n{}", style("Bob 编辑器版本管理器").bold().cyan());
        println!("一个简单的 Vim 和 Neovim 版本管理工具\n");

        let choices = vec!["安装 Vim", "安装 Neovim", "清除缓存", "退出"];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("请选择操作")
            .default(0)
            .items(&choices)
            .interact()?;

        match selection {
            0 => display_version_menu(&manager, EditorType::Vim).await?,
            1 => display_version_menu(&manager, EditorType::Neovim).await?,
            2 => manager.clear_cache().await?,
            3 => {
                println!("感谢使用，祝您编码愉快！");
                break;
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = main_menu().await {
        eprintln!("错误: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
