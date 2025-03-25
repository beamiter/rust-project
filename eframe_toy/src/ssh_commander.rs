use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct SSHCommander {
    host: String,
    username: String,
    password: String,
    show_password: bool,
    ddp_time: String,
    product: String,
    bag: String,
    #[serde(skip)]
    output: String,
    #[serde(skip)]
    show_file_dialog: bool,
    current_directory: String,
    #[serde(skip)]
    directory_contents: Vec<String>,
    #[serde(skip)]
    is_loading_directory: bool,
    #[serde(skip)]
    ssh_connection: Option<Arc<Mutex<SSHConnection>>>,
    #[serde(skip)]
    output_command: String,
    #[serde(skip)]
    need_execute: bool,
}
struct SSHConnection {
    session: Session,
    last_used: Instant,
}

const COMMAND_PREFIX: &str = "docker exec fpp-container-mnt-data-maf_planning ./sim fpp play -v ";

impl Default for SSHCommander {
    fn default() -> Self {
        Self {
            host: "10.21.31.17:22".into(),
            username: "root".into(),
            password: "1".into(),
            show_password: false,
            bag: "/opt/maf_planning/backup/bags".into(),
            ddp_time: "2.0".into(),
            product: "MNP".into(),
            output: String::new(),
            output_command: String::new(),
            show_file_dialog: false,
            current_directory: "/opt/maf_planning/backup/bags".into(),
            directory_contents: Vec::new(),
            is_loading_directory: false,
            ssh_connection: None,
            need_execute: false,
        }
    }
}

use ssh2::Session;
use std::{io::Read, net::TcpStream};

impl SSHCommander {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }
    fn load_directory_contents(&mut self) {
        self.is_loading_directory = true;
        let path = self.current_directory.clone();
        match self.list_remote_directory(&path) {
            Ok(contents) => self.directory_contents = contents,
            Err(e) => {
                println!("Error listing directory: {}", e);
                self.directory_contents = vec![format!("Error: {}", e)];
            }
        }
        self.is_loading_directory = false;
        // // 克隆以在异步闭包中使用
        // let path = self.current_directory.clone();
        // let host = self.host.clone();
        // let username = self.username.clone();
        // let password = self.password.clone();
        // // 这里假设有一个异步运行时，实际应用中可能需要额外配置
        // std::thread::spawn(move || {
        //     let mut commander = SSHCommander {
        //         host,
        //         username,
        //         password,
        //         ..Default::default()
        //     };
        //     match commander.list_remote_directory(&path) {
        //         Ok(contents) => commander.directory_contents = contents,
        //         Err(e) => {
        //             println!("Error listing directory: {}", e);
        //             commander.directory_contents = vec![format!("Error: {}", e)];
        //         }
        //     }
        //     commander.is_loading_directory = false;
        //     // 使用某种方式将结果发送回主线程
        //     // 这需要额外的同步机制，比如channel或Mutex
        // });
    }
    fn ensure_ssh_connection(
        &mut self,
    ) -> Result<Arc<Mutex<SSHConnection>>, Box<dyn std::error::Error>> {
        // 如果连接存在且最近有使用，直接返回

        if let Some(conn) = &self.ssh_connection {
            let mut connection = conn.lock().unwrap();

            // 检查连接是否超过5分钟未使用，如果是则刷新

            if connection.last_used.elapsed() > Duration::from_secs(300) {
                // 发送一个空命令保持连接活跃

                let mut channel = connection.session.channel_session()?;

                channel.exec("echo")?;

                channel.wait_close()?;
            }

            connection.last_used = Instant::now();

            return Ok(conn.clone());
        }

        // 创建新连接

        let tcp = TcpStream::connect(&self.host)?;

        let mut sess = Session::new()?;

        sess.set_tcp_stream(tcp);

        sess.handshake()?;

        sess.userauth_password(&self.username, &self.password)?;

        if !sess.authenticated() {
            return Err("Authentication failed".into());
        }

        let connection = Arc::new(Mutex::new(SSHConnection {
            session: sess,

            last_used: Instant::now(),
        }));

        self.ssh_connection = Some(connection.clone());

        Ok(connection)
    }
    fn list_remote_directory(
        &mut self,
        path: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // 获取SSH连接
        let connection = self.ensure_ssh_connection()?;
        let session = &connection.lock().unwrap().session;
        // 创建通道并执行命令
        let mut channel = session.channel_session()?;
        let command = format!(
            "docker exec fpp-container-mnt-data-maf_planning ls -la {}",
            path
        );
        channel.exec(&command)?;
        // 读取输出
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        // 解析目录内容，跳过前两行(. 和 ..)
        let entries = output
            .lines()
            .skip(1) // 跳过表头行
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 9 {
                    let file_type = parts[0].chars().next();
                    let name = parts[8..].join(" ");
                    // 忽略 . 和 ..
                    if name == "." || name == ".." {
                        return None;
                    }
                    // 为目录添加/后缀
                    if file_type == Some('d') {
                        return Some(format!("{}/", name));
                    } else {
                        return Some(name);
                    }
                }
                None
            })
            .collect();

        Ok(entries)
    }

    fn execute_command(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        // 获取SSH连接
        let connection = self.ensure_ssh_connection()?;
        let session = &connection.lock().unwrap().session;
        // 创建通道并执行命令
        let mut channel = session.channel_session()?;
        channel.exec(&self.output_command)?;
        // 读取输出
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;

        Ok(output)
    }
}

pub fn parse_hyperlink(line: &str) -> Option<String> {
    let url_regex = regex::Regex::new(r"https?://[^\s]+").unwrap();
    if let Some(url_match) = url_regex.find(line) {
        return Some(url_match.as_str().to_string());
    }
    None
}

impl eframe::App for SSHCommander {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:

            egui::menu::bar(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.add_space(16.0);
                }

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("SSH Commander");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Host: ");
                ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(150.));
            });
            ui.horizontal(|ui| {
                ui.label("Username: ");
                ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(100.));
                ui.label("Password: ");
                if self.show_password {
                    ui.add(egui::TextEdit::singleline(&mut self.password).desired_width(100.));
                } else {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .desired_width(100.),
                    );
                }
                if ui
                    .button(if self.show_password { "Hide" } else { "Show" })
                    .clicked()
                {
                    self.show_password = !self.show_password;
                }
            });
            ui.horizontal(|ui| {
                ui.label("ddp_time: ");
                ui.add(egui::TextEdit::singleline(&mut self.ddp_time).desired_width(100.));
                ui.label("product: ");
                ui.add(egui::TextEdit::singleline(&mut self.product).desired_width(100.));
            });
            ui.horizontal(|ui| {
                ui.label("bag: ");
                let remaining_width = ui.available_width() - 100.0; // 为按钮保留空间
                ui.add(egui::TextEdit::singleline(&mut self.bag).desired_width(remaining_width));
                if ui.button("Browse...").clicked() {
                    self.show_file_dialog = true;
                    self.load_directory_contents(); // 加载目录内容
                }
            });
            if self.show_file_dialog {
                egui::Window::new("Select Bag File")
                    .fixed_size([400.0, 300.0])
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Current directory: ");
                            ui.text_edit_singleline(&mut self.current_directory);
                            if ui.button("Go").clicked() {
                                self.load_directory_contents();
                            }
                        });
                        ui.separator();
                        if self.is_loading_directory {
                            ui.spinner();
                            ui.label("Loading directory contents...");
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                // 显示"向上"选项
                                if ui.selectable_label(false, "..").clicked() {
                                    // 获取父目录路径
                                    if let Some(parent_end) = self.current_directory.rfind('/') {
                                        if parent_end > 0 {
                                            self.current_directory =
                                                self.current_directory[..parent_end].to_string();
                                            self.load_directory_contents();
                                        }
                                    }
                                }
                                // 显示目录内容
                                for item in &self.directory_contents.clone() {
                                    let is_dir = item.ends_with('/');
                                    if ui.selectable_label(false, item).clicked() {
                                        if is_dir {
                                            // 导航到子目录
                                            self.current_directory = format!(
                                                "{}/{}",
                                                self.current_directory,
                                                item.trim_end_matches('/')
                                            );
                                            self.load_directory_contents();
                                        } else {
                                            // 选择文件
                                            self.bag =
                                                format!("{}/{}", self.current_directory, item);
                                            self.show_file_dialog = false;
                                        }
                                    }
                                }
                            });
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.show_file_dialog = false;
                            }
                        });
                    });
            }
            if self.need_execute {
                self.need_execute = false;
                match self.execute_command() {
                    Ok(output) => self.output = output,
                    Err(e) => self.output = format!("Error executing command: {}", e),
                }
            }
            if ui.button("Execute").clicked() {
                self.need_execute = true;
                self.output_command = COMMAND_PREFIX.to_owned()
                    + "--ddp-time "
                    + &self.ddp_time
                    + " --product "
                    + &self.product
                    + " "
                    + &self.bag;
                println!("command: {}", self.output_command);
            }
            ui.label(&self.output_command);
            ui.separator();
            ui.heading("Output:");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in self.output.lines() {
                    if let Some(link) = parse_hyperlink(line) {
                        ui.hyperlink_to(line, link);
                    } else {
                        ui.label(line);
                    }
                }
            });
            ui.separator();
            // ui.add(egui::github_link_file!(
            //     "https://github.com/emilk/eframe_template/blob/main/",
            //     "Source code."
            // ));
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                powered_by_egui_and_eframe(ui);
                egui::warn_if_debug_build(ui);
            });
        });
        if self.need_execute {
            ctx.request_repaint();
        }
    }
}

fn powered_by_egui_and_eframe(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label("Powered by ");
        ui.hyperlink_to("egui", "https://github.com/emilk/egui");
        ui.label(" and ");
        ui.hyperlink_to(
            "eframe",
            "https://github.com/emilk/egui/tree/master/crates/eframe",
        );
        ui.label(".");
    });
}
