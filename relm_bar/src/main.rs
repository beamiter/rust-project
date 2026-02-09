use chrono::Local;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4::glib::ControlFlow;
use log::{error, info, warn};
use relm4::{ComponentParts, ComponentSender, RelmApp, SimpleComponent};

use std::sync::Arc;
use std::time::Duration;

use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use xbar_core::initialize_logging;
use xbar_core::system_monitor::SystemMonitor;

// ========== 工具与常量 ==========

fn pick_emoji(i: usize) -> &'static str {
    match i {
        0 => "🍜",
        1 => "🎨",
        2 => "🍀",
        3 => "🧿",
        4 => "🌟",
        5 => "🐐",
        6 => "🏆",
        7 => "🕊️",
        8 => "🏡",
        _ => "❔",
    }
}

fn monitor_num_to_icon(monitor_num: u8) -> String {
    match monitor_num {
        0 => "🥇".to_string(),
        1 => "🥈".to_string(),
        2 => "🥉".to_string(),
        _ => "❔".to_string(),
    }
}

// 用于 Tab 状态样式
fn compute_tab_css_classes(s: Option<&TagStatus>) -> Vec<&'static str> {
    match s {
        Some(st) if st.is_urg => vec!["tab-button", "urgent"],
        Some(st) if st.is_filled => vec!["tab-button", "filled"],
        Some(st) if st.is_selected => vec!["tab-button", "selected"],
        Some(st) if st.is_occ => vec!["tab-button", "occupied"],
        _ => vec!["tab-button", "empty"],
    }
}

// 设置指标等级类
fn metric_level_class(usage: f64) -> &'static str {
    if usage < 0.50 {
        "level-ok"
    } else if usage < 0.70 {
        "level-warn"
    } else if usage < 0.85 {
        "level-high"
    } else {
        "level-crit"
    }
}

// 将指标类应用到某个 Widget（清除旧等级类后再添加新的）
fn apply_metric_classes<W: IsA<gtk::Widget>>(w: &W, usage: f64) {
    static LEVELS: [&str; 4] = ["level-ok", "level-warn", "level-high", "level-crit"];
    let widget = w.as_ref();
    for c in LEVELS {
        widget.remove_css_class(c);
    }
    widget.add_css_class(metric_level_class(usage));
}

// 应用 Tab 状态类
fn apply_tab_state_classes(button: &gtk::Button, status: Option<&TagStatus>) {
    static TAB_STATES: [&str; 5] = ["urgent", "filled", "selected", "occupied", "empty"];
    let w = button.upcast_ref::<gtk::Widget>();
    // 保留 tab-button，不清除
    for s in TAB_STATES {
        w.remove_css_class(s);
    }
    for c in compute_tab_css_classes(status) {
        w.add_css_class(c);
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(include_str!("styles.css"));
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

// ========== App 定义 ==========

#[derive(Debug)]
pub enum AppInput {
    TabSelected(usize),
    LayoutChanged(u32),
    ToggleLayoutPanel,
    ToggleSeconds,
    ToggleTheme,
    Screenshot,
    SharedMessageReceived(SharedMessage),
    SystemUpdate,
    UpdateTime,
}

#[tracker::track]
pub struct AppModel {
    pub active_tab: usize,
    pub layout_symbol: String,
    pub layout_open: bool,
    pub monitor_num: u8,
    pub show_seconds: bool,
    pub theme_dark: bool,
    pub tag_status_vec: Vec<TagStatus>,
    pub last_shared_message: Option<SharedMessage>,
    pub memory_usage: f64,
    pub cpu_usage: f64,
    pub current_time: String,

    #[do_not_track]
    shared_buffer_opt: Option<Arc<SharedRingBuffer>>,
    #[do_not_track]
    pub system_monitor: SystemMonitor,

    // 来自 UI 的控件引用
    #[do_not_track]
    cpu_label_widget: gtk::Label,
    #[do_not_track]
    memory_label_widget: gtk::Label,
    #[do_not_track]
    time_button_widget: gtk::Button,
    #[do_not_track]
    theme_button_widget: gtk::Button,
    #[do_not_track]
    monitor_label_widget: gtk::Label,
    #[do_not_track]
    tab_buttons: Vec<gtk::Button>,

    #[do_not_track]
    root_window: gtk::ApplicationWindow,

    // 新增：布局开关与选项（与 gtk_bar 的 UI/样式一致）
    #[do_not_track]
    layout_toggle_widget: gtk::Button,
    #[do_not_track]
    layout_revealer_widget: gtk::Revealer,
    #[do_not_track]
    layout_btn_tiled_widget: gtk::Button,
    #[do_not_track]
    layout_btn_floating_widget: gtk::Button,
    #[do_not_track]
    layout_btn_monocle_widget: gtk::Button,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = String; // 共享路径
    type Input = AppInput;
    type Output = ();

    view! {
        #[root]
        gtk::ApplicationWindow {
            set_decorated: false,
            set_default_size: (1000, 40),
            set_resizable: true,
            add_css_class: "main-window",

            // 将 UI 文件中的 top_hbox 作为唯一子控件挂载进来
            set_child: Some(&top_hbox),
        }
    }

    fn init(
        shared_path: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // 1) 加载 UI 文件（复用 gtk_bar 的 main_layout.ui）
        let builder = gtk::Builder::from_string(include_str!("resources/main_layout.ui"));

        // 2) 取出要挂载的根容器（不是 UI 的窗口，避免重复窗口）
        let top_hbox: gtk::Box = builder
            .object("top_hbox")
            .expect("Missing top_hbox in UI file");
        top_hbox.unparent();

        // 3) 获取需要动态更新的控件
        let cpu_label_widget: gtk::Label = builder.object("cpu_label").expect("Missing cpu_label");
        cpu_label_widget.add_css_class("metric-label");
        let memory_label_widget: gtk::Label = builder
            .object("memory_label")
            .expect("Missing memory_label");
        memory_label_widget.add_css_class("metric-label");
        let time_button_widget: gtk::Button =
            builder.object("time_label").expect("Missing time_label");
        let theme_button_widget: gtk::Button = builder
            .object("theme_button")
            .expect("Missing theme_button");
        let monitor_label_widget: gtk::Label = builder
            .object("monitor_label")
            .expect("Missing monitor_label");

        // 布局开关 + 选项（复用 gtk_bar 定义）
        let layout_toggle_widget: gtk::Button = builder
            .object("layout_toggle")
            .expect("Missing layout_toggle");
        let layout_revealer_widget: gtk::Revealer = builder
            .object("layout_revealer")
            .expect("Missing layout_revealer");
        let layout_btn_tiled_widget: gtk::Button = builder
            .object("layout_option_tiled")
            .expect("Missing layout_option_tiled");
        let layout_btn_floating_widget: gtk::Button = builder
            .object("layout_option_floating")
            .expect("Missing layout_option_floating");
        let layout_btn_monocle_widget: gtk::Button = builder
            .object("layout_option_monocle")
            .expect("Missing layout_option_monocle");

        // 4) 连接静态按钮的信号
        // 布局开关
        {
            let s = sender.clone();
            layout_toggle_widget.connect_clicked(move |_| s.input(AppInput::ToggleLayoutPanel));
        }
        // 布局选项
        {
            let s = sender.clone();
            layout_btn_tiled_widget.connect_clicked(move |_| s.input(AppInput::LayoutChanged(0)));
        }
        {
            let s = sender.clone();
            layout_btn_floating_widget
                .connect_clicked(move |_| s.input(AppInput::LayoutChanged(1)));
        }
        {
            let s = sender.clone();
            layout_btn_monocle_widget.connect_clicked(move |_| s.input(AppInput::LayoutChanged(2)));
        }

        // 截图按钮
        if let Some(btn) = builder.object::<gtk::Button>("screenshot_button") {
            let s = sender.clone();
            btn.connect_clicked(move |_| s.input(AppInput::Screenshot));
        }

        // 时间按钮
        {
            let s = sender.clone();
            time_button_widget.connect_clicked(move |_| s.input(AppInput::ToggleSeconds));
        }

        // 主题按钮
        {
            let s = sender.clone();
            theme_button_widget.connect_clicked(move |_| s.input(AppInput::ToggleTheme));
        }

        // Tab 按钮与初始 emoji
        let mut tab_buttons = Vec::with_capacity(9);
        for i in 0..9 {
            let id = format!("tab_button_{}", i);
            if let Some(btn) = builder.object::<gtk::Button>(&id) {
                btn.set_label(pick_emoji(i));
                let s = sender.clone();
                btn.connect_clicked(move |_| s.input(AppInput::TabSelected(i)));
                tab_buttons.push(btn);
            } else {
                warn!("Missing {}", id);
            }
        }

        // 5) 构建 model
        let shared_arc =
            SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
        let mut model = AppModel {
            active_tab: 0,
            layout_symbol: "[]=".to_string(),
            layout_open: false,
            monitor_num: 0,
            show_seconds: false,
            theme_dark: true,
            tag_status_vec: Vec::new(),
            last_shared_message: None,
            memory_usage: 0.0,
            cpu_usage: 0.0,
            current_time: "".to_string(),
            shared_buffer_opt: shared_arc.clone(),
            system_monitor: SystemMonitor::new(1),
            tracker: 0,

            cpu_label_widget,
            memory_label_widget,
            time_button_widget,
            theme_button_widget,
            monitor_label_widget,
            tab_buttons,

            root_window: root.clone(),

            layout_toggle_widget,
            layout_revealer_widget,
            layout_btn_tiled_widget,
            layout_btn_floating_widget,
            layout_btn_monocle_widget,
        };

        // 6) 样式、首帧数据与后台任务
        load_css();

        // Match gtk_bar defaults: use dark theme by default so window/theme styles apply.
        root.add_css_class("theme-dark");
        root.remove_css_class("theme-light");

        model.update_time_display();

        // 先把 UI 设为初始状态
        model.sync_full_ui_once();

        // 定时器与共享线程
        spawn_background_tasks(sender.clone(), shared_arc);

        // 触发一次系统监控刷新
        sender.input(AppInput::SystemUpdate);

        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        self.reset();
        match msg {
            AppInput::TabSelected(index) => {
                info!("Tab selected: {}", index);
                self.active_tab = index;
                self.send_tag_command(true);
                self.sync_tabs_ui();
            }

            AppInput::LayoutChanged(layout_index) => {
                info!("Layout changed: {}", layout_index);
                self.send_layout_command(layout_index);
                // 选择后收起，并刷新布局 UI（高亮 current）
                self.layout_open = false;
                self.sync_layout_ui();
            }

            AppInput::ToggleLayoutPanel => {
                self.layout_open = !self.layout_open;
                self.sync_layout_ui();
            }

            AppInput::ToggleSeconds => {
                self.show_seconds = !self.show_seconds;
                self.update_time_display();
                self.sync_time_ui();
            }

            AppInput::ToggleTheme => {
                self.theme_dark = !self.theme_dark;
                self.sync_theme_ui();
            }

            AppInput::Screenshot => {
                info!("Taking screenshot");
                if let Err(e) = std::process::Command::new("flameshot").arg("gui").spawn() {
                    error!("Failed to launch flameshot: {}", e);
                }
            }

            AppInput::SharedMessageReceived(message) => {
                info!("SharedMessageReceived: {:?}", message);
                self.process_shared_message(message);
                // 刷新 tab、布局开关与选项、监视器图标
                self.sync_tabs_ui();
                self.sync_layout_and_monitor_ui();
            }

            AppInput::SystemUpdate => {
                self.system_monitor.update_if_needed();
                if let Some(snapshot) = self.system_monitor.get_snapshot() {
                    let total = snapshot.memory_available + snapshot.memory_used;
                    self.memory_usage = if total > 0 {
                        snapshot.memory_used as f64 / total as f64
                    } else {
                        0.0
                    };
                    self.cpu_usage = (snapshot.cpu_average as f64 / 100.0).clamp(0.0, 1.0);
                }
                self.sync_metrics_ui();
            }

            AppInput::UpdateTime => {
                self.update_time_display();
                self.sync_time_ui();
            }
        }
    }
}

// ========== AppModel 实现 ==========

impl AppModel {
    fn update_time_display(&mut self) {
        let now = Local::now();
        let format_str = if self.show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        self.current_time = now.format(format_str).to_string();
    }

    fn send_tag_command(&self, is_view: bool) {
        if let Some(shared_buffer) = &self.shared_buffer_opt {
            if let Some(ref message) = self.last_shared_message {
                let command = if is_view {
                    SharedCommand::view_tag(1 << self.active_tab, message.monitor_info.monitor_num)
                } else {
                    SharedCommand::toggle_tag(
                        1 << self.active_tab,
                        message.monitor_info.monitor_num,
                    )
                };
                if let Err(e) = shared_buffer.send_command(command) {
                    error!("Failed to send tag command: {}", e);
                }
            }
        }
    }

    fn send_layout_command(&self, layout_index: u32) {
        if let Some(shared_buffer) = &self.shared_buffer_opt {
            if let Some(ref message) = self.last_shared_message {
                let monitor_id = message.monitor_info.monitor_num;
                let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
                if let Err(e) = shared_buffer.send_command(command) {
                    error!("Failed to send layout command: {}", e);
                }
            }
        }
    }

    fn process_shared_message(&mut self, message: SharedMessage) {
        self.last_shared_message = Some(message.clone());
        self.layout_symbol = message.monitor_info.get_ltsymbol();
        self.monitor_num = message.monitor_info.monitor_num as u8;
        self.set_tag_status_vec(message.monitor_info.tag_status_vec.to_vec());

        // 更新活动标签
        for (index, tag_status) in message.monitor_info.tag_status_vec.iter().enumerate() {
            if tag_status.is_selected {
                self.active_tab = index;
            }
        }
    }

    // ========== UI 同步 ==========

    fn sync_full_ui_once(&self) {
        self.sync_layout_and_monitor_ui();
        self.sync_theme_ui();
        self.sync_time_ui();
        self.sync_metrics_ui();
        self.sync_tabs_ui();
    }

    // 布局 + 监视器图标
    fn sync_layout_and_monitor_ui(&self) {
        // 布局开关：文本为当前布局
        self.layout_toggle_widget.set_label(&self.layout_symbol);
        // 根据 layout_open 切换 open/closed 类，并控制 revealer 展开
        self.sync_layout_open_state_ui();

        // 当前布局选项高亮
        self.sync_layout_current_option_ui();

        // 监视器图标
        self.monitor_label_widget
            .set_label(&monitor_num_to_icon(self.monitor_num));
    }

    // 仅同步布局开关的 open/closed 与 revealer 展开状态
    fn sync_layout_open_state_ui(&self) {
        let w = self.layout_toggle_widget.upcast_ref::<gtk::Widget>();
        w.remove_css_class("open");
        w.remove_css_class("closed");
        w.add_css_class(if self.layout_open { "open" } else { "closed" });
        self.layout_revealer_widget
            .set_reveal_child(self.layout_open);
    }

    // 根据 layout_symbol 高亮当前布局选项
    fn sync_layout_current_option_ui(&self) {
        let tiled = self.layout_symbol.contains("[]=");
        let floating = self.layout_symbol.contains("><>");
        let monocle = self.layout_symbol.contains("[M]");

        for b in [
            &self.layout_btn_tiled_widget,
            &self.layout_btn_floating_widget,
            &self.layout_btn_monocle_widget,
        ] {
            b.remove_css_class("current");
        }
        if tiled {
            self.layout_btn_tiled_widget.add_css_class("current");
        } else if floating {
            self.layout_btn_floating_widget.add_css_class("current");
        } else if monocle {
            self.layout_btn_monocle_widget.add_css_class("current");
        }
    }

    // 仅在 ToggleLayoutPanel 或 LayoutChanged 后调用
    fn sync_layout_ui(&self) {
        // 更新 toggle 文本为当前布局
        self.layout_toggle_widget.set_label(&self.layout_symbol);
        // 更新开关样式与 revealer 展开
        self.sync_layout_open_state_ui();
        // 高亮当前选项
        self.sync_layout_current_option_ui();
    }

    fn sync_time_ui(&self) {
        self.time_button_widget.set_label(&self.current_time);
    }

    fn sync_metrics_ui(&self) {
        // 与 UI 文件一致：仅显示百分比
        let cpu_pct = (self.cpu_usage * 100.0).round() as u32;
        let mem_pct = (self.memory_usage * 100.0).round() as u32;
        self.cpu_label_widget
            .set_label(&format!("CPU {:>3}%", cpu_pct));
        self.memory_label_widget
            .set_label(&format!("MEM {:>3}%", mem_pct));

        // 应用等级类
        apply_metric_classes(&self.cpu_label_widget, self.cpu_usage);
        apply_metric_classes(&self.memory_label_widget, self.memory_usage);
    }

    fn sync_tabs_ui(&self) {
        for (i, btn) in self.tab_buttons.iter().enumerate() {
            btn.set_label(pick_emoji(i));
            let status = self.tag_status_vec.get(i);
            apply_tab_state_classes(btn, status);
        }
    }

    fn sync_theme_ui(&self) {
        if self.theme_dark {
            self.root_window.add_css_class("theme-dark");
            self.root_window.remove_css_class("theme-light");
            self.theme_button_widget.set_label(" 🌙 ");
        } else {
            self.root_window.add_css_class("theme-light");
            self.root_window.remove_css_class("theme-dark");
            self.theme_button_widget.set_label(" ☀️ ");
        }
    }
}

// ========== 后台任务 ==========

fn spawn_background_tasks(
    sender: ComponentSender<AppModel>,
    shared_buffer: Option<Arc<SharedRingBuffer>>,
) {
    // 系统监控任务（每2秒）
    let sender1 = sender.clone();
    glib::timeout_add_seconds_local(2, move || {
        sender1.input(AppInput::SystemUpdate);
        ControlFlow::Continue
    });

    // 时间更新任务（每1秒）
    let sender2 = sender.clone();
    glib::timeout_add_seconds_local(1, move || {
        sender2.input(AppInput::UpdateTime);
        ControlFlow::Continue
    });

    // 共享内存任务
    // 共享内存任务：仅当有 shared 才启动线程
    if let Some(shared_buffer) = shared_buffer {
        let sender3 = sender.clone();
        std::thread::spawn(move || {
            shared_memory_worker(shared_buffer, sender3);
        });
    } else {
        log::warn!("No shared buffer, shared memory worker not started");
    }
}

fn shared_memory_worker(shared: Arc<SharedRingBuffer>, sender: ComponentSender<AppModel>) {
    info!("Starting shared memory worker");

    let mut prev_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    loop {
        match shared.wait_for_message(Some(Duration::from_secs(2))) {
            Ok(true) => {
                if let Ok(Some(message)) = shared.try_read_latest_message() {
                    if prev_timestamp != message.timestamp.into() {
                        prev_timestamp = message.timestamp.into();
                        sender.input(AppInput::SharedMessageReceived(message));
                    }
                }
            }
            Ok(false) => log::debug!("[notifier] Wait for message timed out."),
            Err(e) => {
                error!("[notifier] Wait for message failed: {}", e);
                break;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging("relm_bar", &shared_path) {
        eprintln!("Init logging failed: {e}");
    }

    let app = RelmApp::new("dev.relm.bar").with_args(vec![]);

    app.run::<AppModel>(shared_path);
}
