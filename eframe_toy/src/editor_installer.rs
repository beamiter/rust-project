use eframe::egui;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use chrono::{DateTime, NaiveDateTime};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_LENGTH;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct VersionInfo {
    version: String,
    url: String,
    published_at: Option<String>,
    description: Option<String>,
}
impl VersionInfo {
    fn get_date(&self) -> Option<NaiveDateTime> {
        self.published_at.as_ref().and_then(|date_str| {
            DateTime::parse_from_rfc3339(date_str.as_str())
                .map(|dt| dt.naive_utc())
                .ok()
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
enum EditorType {
    Vim,
    Neovim,
}
impl EditorType {
    #[allow(dead_code)]
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

#[derive(Clone)]
struct HttpManager {
    client: Client,
}
impl Default for HttpManager {
    fn default() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("eframe_toy-editor-installer")
            .build()
            .expect("Failed to build HTTP client");
        Self { client }
    }
}
impl HttpManager {
    fn fetch_versions(&self, editor: EditorType) -> Result<Vec<VersionInfo>, String> {
        let url = editor.api_url();
        let releases: serde_json::Value = self
            .client
            .get(url)
            .send()
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| format!("parse GitHub API failed: {e}"))?;

        let version_regex = Regex::new(r"releases/download/(.*?)/").unwrap();
        let mut versions = Vec::new();

        if let Some(releases_array) = releases.as_array() {
            for release in releases_array.iter().take(15) {
                let published_at = release["published_at"].as_str().map(String::from);
                let description = release["body"].as_str().map(String::from);

                if let Some(assets) = release["assets"].as_array() {
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

        // 排序：优先按日期降序，再按版本字符串
        versions.sort_by(|a, b| match (a.get_date(), b.get_date()) {
            (Some(da), Some(db)) => db.cmp(&da),
            _ => b.version.cmp(&a.version),
        });

        Ok(versions)
    }

    fn head_content_length(&self, url: &str) -> Option<u64> {
        self.client.head(url).send().ok().and_then(|r| {
            r.headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
        })
    }

    fn download_to(
        &self,
        url: &str,
        dest: &Path,
        progress_tx: Sender<ProgressMsg>,
    ) -> Result<(), String> {
        let total = self.head_content_length(url).unwrap_or(0);
        let _ = progress_tx.send(ProgressMsg::Init { total });

        let mut resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("download request failed: {e}"))?;

        let mut file = std::fs::File::create(dest)
            .map_err(|e| format!("create file error {}: {}", dest.display(), e))?;

        let mut downloaded: u64 = 0;
        let mut buf = [0u8; 64 * 1024];

        loop {
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            downloaded += n as u64;
            let _ = progress_tx.send(ProgressMsg::Progress { downloaded, total });
        }

        // 置可执行权限（UNIX）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(dest).map_err(|e| e.to_string())?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(dest, perms).map_err(|e| e.to_string())?;
        }

        let _ = progress_tx.send(ProgressMsg::Finished {
            path: dest.to_path_buf(),
        });
        Ok(())
    }
}

#[derive(Debug)]
enum ProgressMsg {
    Init { total: u64 },
    Progress { downloaded: u64, total: u64 },
    Finished { path: PathBuf },
    Error(String),
}

#[derive(Debug)]
#[allow(dead_code)]
enum InstallMsg {
    Started,
    Log(String),
    Finished {
        success: bool,
        log: String,
        verify: Option<String>,
        error: Option<String>,
    },
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct EditorsInstallerApp {
    #[serde(skip)]
    http: HttpManager,

    editor: EditorType,

    #[serde(skip)]
    versions_rx: Option<Receiver<Result<Vec<VersionInfo>, String>>>,
    #[serde(skip)]
    versions_loading: bool,
    #[serde(skip)]
    versions: Vec<VersionInfo>,
    #[serde(skip)]
    versions_error: Option<String>,

    #[serde(skip)]
    selected_index: Option<usize>,

    // 下载相关
    #[serde(skip)]
    download_rx: Option<Receiver<ProgressMsg>>,
    #[serde(skip)]
    downloading: bool,
    #[serde(skip)]
    downloaded_bytes: u64,
    #[serde(skip)]
    total_bytes: Option<u64>,
    #[serde(skip)]
    temp_download_path: Option<PathBuf>,

    // 安装相关
    install_system_wide: bool, // true => /usr/local/bin, false => ~/.local/bin
    #[serde(skip)]
    installing: bool,
    #[serde(skip)]
    install_log: String,
    #[serde(skip)]
    verify_output: Option<String>,
    #[serde(skip)]
    error_msg: Option<String>,

    // 新增：安装消息接收器
    #[serde(skip)]
    install_rx: Option<Receiver<InstallMsg>>,
}

impl Default for EditorsInstallerApp {
    fn default() -> Self {
        Self {
            http: HttpManager::default(),
            editor: EditorType::Neovim, // 默认选 Neovim

            versions_rx: None,
            versions_loading: false,
            versions: Vec::new(),
            versions_error: None,

            selected_index: None,

            download_rx: None,
            downloading: false,
            downloaded_bytes: 0,
            total_bytes: None,
            temp_download_path: None,

            install_system_wide: false,
            installing: false,
            install_log: String::new(),
            verify_output: None,
            error_msg: None,

            install_rx: None,
        }
    }
}

impl EditorsInstallerApp {
    fn trigger_fetch_versions(&mut self) {
        if self.versions_loading {
            return;
        }
        self.versions_loading = true;
        self.versions_error = None;
        self.versions.clear();
        self.selected_index = None;

        let (tx, rx) = channel();
        self.versions_rx = Some(rx);
        let http = self.http.clone();
        let editor = self.editor;

        thread::spawn(move || {
            let res = http.fetch_versions(editor);
            let _ = tx.send(res);
        });
    }

    fn trigger_download(&mut self) {
        if self.downloading {
            return;
        }
        self.error_msg = None;
        self.verify_output = None;

        let Some(idx) = self.selected_index else {
            return;
        };
        let Some(version) = self.versions.get(idx) else {
            return;
        };

        self.downloading = true;
        self.downloaded_bytes = 0;
        self.total_bytes = None;

        let (tx, rx) = channel();
        self.download_rx = Some(rx);

        let http = self.http.clone();
        let url = version.url.clone();
        let tmp_path = std::env::temp_dir().join(format!("{}.appimage", self.editor.binary_name()));
        self.temp_download_path = Some(tmp_path.clone());

        thread::spawn(move || {
            if let Err(e) = http.download_to(&url, &tmp_path, tx.clone()) {
                let _ = tx.send(ProgressMsg::Error(e));
            }
        });
    }

    fn install_target_path(&self) -> PathBuf {
        if self.install_system_wide {
            PathBuf::from(format!("/usr/local/bin/{}", self.editor.binary_name()))
        } else {
            // ~/.local/bin
            let home = dirs::home_dir().unwrap_or(std::env::current_dir().unwrap_or_default());
            home.join(".local/bin").join(self.editor.binary_name())
        }
    }

    fn trigger_install(&mut self) {
        if self.installing {
            return;
        }
        self.install_log.clear();
        self.error_msg = None;
        self.verify_output = None;

        let Some(src) = self.temp_download_path.clone() else {
            self.error_msg = Some("no downloaded file to install".into());
            return;
        };
        let dest = self.install_target_path();
        self.installing = true;

        let (tx, rx) = channel();
        self.install_rx = Some(rx);

        let system_wide = self.install_system_wide;

        thread::spawn(move || {
            let _ = tx.send(InstallMsg::Started);

            let mut log = String::new();
            let mut verify: Option<String> = None;
            let error: Option<String>;

            let mut push = |s: &str| {
                if !log.is_empty() {
                    log.push('\n');
                }
                log.push_str(s);
                let _ = tx.send(InstallMsg::Log(s.to_string()));
            };

            push(&format!("Installing to: {}", dest.display()));
            push(&format!("Source file: {}", src.display()));

            // 确保目标目录存在（用户目录）
            if !system_wide {
                if let Some(parent) = dest.parent() {
                    match fs::create_dir_all(parent) {
                        Ok(_) => {
                            push(&format!("Ensured dir exists: {}", parent.display()));
                        }
                        Err(e) => {
                            let msg = format!("create dir failed: {}", e);
                            push(&msg);
                            let _ = tx.send(InstallMsg::Finished {
                                success: false,
                                log,
                                verify: None,
                                error: Some(msg),
                            });
                            return;
                        }
                    }
                }
            }

            let res = if system_wide {
                // 提升权限安装（避免 bash -lc，直接传递参数）
                #[cfg(unix)]
                {
                    // 先尝试 pkexec
                    let pk_status = Command::new("pkexec")
                        .arg("/usr/bin/install")
                        .args([
                            "-Dm755",
                            src.to_str().unwrap_or_default(),
                            dest.to_str().unwrap_or_default(),
                        ])
                        .status();

                    match pk_status {
                        Ok(s) if s.success() => Ok(()),
                        _ => {
                            push("pkexec failed or not available, fallback to sudo...");
                            let sd_status = Command::new("sudo")
                                .arg("/usr/bin/install")
                                .args([
                                    "-Dm755",
                                    src.to_str().unwrap_or_default(),
                                    dest.to_str().unwrap_or_default(),
                                ])
                                .status();
                            match sd_status {
                                Ok(s) if s.success() => Ok(()),
                                Ok(s) => Err(format!("sudo install fail: exit={}", s)),
                                Err(e) => Err(format!("exec sudo fail: {}", e)),
                            }
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    Err("system-wide install is only supported on Unix currently".to_string())
                }
            } else {
                // 用户目录安装：复制 + chmod
                match fs::copy(&src, &dest) {
                    Ok(_) => {
                        push("Copied to user dir.");
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            match fs::metadata(&dest).map(|m| m.permissions()) {
                                Ok(mut perms) => {
                                    perms.set_mode(0o755);
                                    if let Err(e) = fs::set_permissions(&dest, perms) {
                                        let msg = format!("chmod failed: {}", e);
                                        push(&msg);
                                        Err(msg)
                                    } else {
                                        Ok(())
                                    }
                                }
                                Err(e) => {
                                    let msg = format!("metadata failed: {}", e);
                                    push(&msg);
                                    Err(msg)
                                }
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            Ok(())
                        }
                    }
                    Err(e) => {
                        let msg = format!("copy failed: {}", e);
                        push(&msg);
                        Err(msg)
                    }
                }
            };

            match res {
                Ok(()) => {
                    push(&format!("install succeed: {}", dest.display()));
                    // 验证版本
                    match Command::new(&dest).arg("--version").output() {
                        Ok(out) => {
                            if out.status.success() {
                                let s = String::from_utf8_lossy(&out.stdout).to_string();
                                verify = Some(s.clone());
                                push("--- verify output ---");
                                for line in s.lines() {
                                    let _ = tx.send(InstallMsg::Log(line.to_string()));
                                }
                            } else {
                                push("verify failed");
                            }
                        }
                        Err(e) => {
                            push(&format!("cannot exec verify: {}", e));
                        }
                    }

                    let _ = tx.send(InstallMsg::Finished {
                        success: true,
                        log,
                        verify,
                        error: None,
                    });
                }
                Err(e) => {
                    error = Some(e.clone());
                    let _ = tx.send(InstallMsg::Finished {
                        success: false,
                        log,
                        verify,
                        error,
                    });
                }
            }
        });
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        // 版本列表结果
        if let Some(rx) = &self.versions_rx {
            match rx.try_recv() {
                Ok(Ok(vs)) => {
                    self.versions = vs;
                    self.versions_loading = false;
                    self.versions_error = None;
                    self.versions_rx = None;
                }
                Ok(Err(e)) => {
                    self.versions_loading = false;
                    self.versions_error = Some(e);
                    self.versions_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.versions_loading = false;
                    self.versions_error = Some("background thread disconncted".to_string());
                    self.versions_rx = None;
                }
            }
        }

        // 下载进度
        if let Some(rx) = &self.download_rx {
            match rx.try_recv() {
                Ok(ProgressMsg::Init { total }) => {
                    self.total_bytes = Some(total);
                    ctx.request_repaint();
                }
                Ok(ProgressMsg::Progress { downloaded, total }) => {
                    self.downloaded_bytes = downloaded;
                    self.total_bytes = Some(total);
                    ctx.request_repaint();
                }
                Ok(ProgressMsg::Finished { path }) => {
                    self.downloading = false;
                    self.temp_download_path = Some(path);
                    self.download_rx = None;
                }
                Ok(ProgressMsg::Error(e)) => {
                    self.downloading = false;
                    self.error_msg = Some(e);
                    self.download_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.downloading = false;
                    self.error_msg = Some("download thread disconncted".to_string());
                    self.download_rx = None;
                }
            }
        }

        // 安装消息
        if let Some(rx) = &self.install_rx {
            // 尽量多处理几条日志，避免 UI 卡顿
            for _ in 0..32 {
                match rx.try_recv() {
                    Ok(InstallMsg::Started) => {
                        ctx.request_repaint();
                    }
                    Ok(InstallMsg::Log(line)) => {
                        if !self.install_log.is_empty() {
                            self.install_log.push('\n');
                        }
                        self.install_log.push_str(&line);
                        ctx.request_repaint();
                    }
                    Ok(InstallMsg::Finished {
                        success: _,
                        log,
                        verify,
                        error,
                    }) => {
                        self.installing = false; // 关键回写
                        if self.install_log.is_empty() {
                            self.install_log = log;
                        }
                        self.verify_output = verify;
                        self.error_msg = error;
                        self.install_rx = None;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint();
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.installing = false;
                        self.error_msg = Some("install thread disconnected".to_string());
                        self.install_rx = None;
                        break;
                    }
                }
            }
        }
    }
}

impl eframe::App for EditorsInstallerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background(ctx);

        egui::TopBottomPanel::top("editor_installer_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Vim / Neovim installer");

                ui.separator();

                egui::ComboBox::from_label("editor")
                    .selected_text(match self.editor {
                        EditorType::Vim => "Vim",
                        EditorType::Neovim => "Neovim",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.editor, EditorType::Vim, "Vim");
                        ui.selectable_value(&mut self.editor, EditorType::Neovim, "Neovim");
                    });

                if ui.button("fetch versions").clicked() {
                    self.trigger_fetch_versions();
                }

                if self.versions_loading {
                    ui.spinner();
                    ui.label("loading...");
                }

                if let Some(err) = &self.versions_error {
                    ui.colored_label(egui::Color32::RED, err);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("target dir: ");
                ui.radio_value(
                    &mut self.install_system_wide,
                    false,
                    "usr dir (~/.local/bin)",
                );
                ui.radio_value(
                    &mut self.install_system_wide,
                    true,
                    "sys dir (/usr/local/bin)",
                );
            });

            ui.separator();

            ui.columns(2, |cols| {
                // 左列：版本列表
                cols[0].heading("available versions");
                egui::ScrollArea::vertical().show(&mut cols[0], |ui| {
                    for (i, v) in self.versions.iter().enumerate() {
                        let date = v
                            .get_date()
                            .map(|d| d.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "unknown date".into());
                        let label = format!("{} ({})", v.version, date);
                        let selected = Some(i) == self.selected_index;

                        if ui.selectable_label(selected, label).clicked() {
                            self.selected_index = Some(i);
                        }
                    }
                });

                // 右列：详情/操作
                cols[1].heading("details");
                if let Some(idx) = self.selected_index {
                    if let Some(v) = self.versions.get(idx) {
                        cols[1].label(format!("version: {}", v.version));
                        if let Some(date) = v.get_date() {
                            cols[1].label(format!(
                                "release date: {}",
                                date.format("%Y-%m-%d %H:%M:%S")
                            ));
                        }
                        cols[1].separator();
                        cols[1].label("release note:");
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(&mut cols[1], |ui| {
                                ui.label(v.description.as_deref().unwrap_or("<None>"));
                            });

                        cols[1].separator();

                        if !self.downloading && self.temp_download_path.is_none() {
                            if cols[1].button("download").clicked() {
                                self.trigger_download();
                            }
                        }

                        if self.downloading {
                            // 进度条
                            let total = self.total_bytes.unwrap_or(0);
                            let frac = if total > 0 {
                                (self.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            cols[1].label(format!(
                                "downloaded: {}/{} bytes",
                                self.downloaded_bytes, total
                            ));
                            cols[1].add(egui::ProgressBar::new(frac).show_percentage());
                        }

                        if let Some(tmp) = &self.temp_download_path {
                            cols[1].label(format!("downloaded: {}", tmp.display()));
                            if !self.installing && cols[1].button("install").clicked() {
                                self.trigger_install();
                            }
                        }

                        if self.installing {
                            cols[1].horizontal(|ui| {
                                ui.spinner();
                                ui.label("installing ...");
                            });
                        }
                    }
                } else {
                    cols[1].label("please select a version on the left");
                }
            });

            if let Some(e) = &self.error_msg {
                ui.separator();
                ui.colored_label(egui::Color32::RED, e);
            }

            // 目标路径提示
            ui.separator();
            ui.label(format!(
                "target path: {}",
                self.install_target_path().display()
            ));
            if !self.install_system_wide {
                ui.label("hint: make sure ~/.local/bin in PATH");
            }
        });

        // 底部日志与验证输出（集中显示所有日志，不再使用 println）
        egui::TopBottomPanel::bottom("install_log_panel").show(ctx, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("install log");
                if ui.button("clear").clicked() {
                    self.install_log.clear();
                }
            });
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.install_log.is_empty() {
                        ui.label("<no log>");
                    } else {
                        ui.code(&self.install_log);
                    }
                });

            if let Some(v) = &self.verify_output {
                ui.separator();
                ui.label("verify output");
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .show(ui, |ui| {
                        ui.code(v);
                    });
            }
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}
