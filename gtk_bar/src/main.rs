use chrono::Local;
use gdk4::prelude::*;
#[cfg(all(target_os = "linux", feature = "x11"))]
use gdk4_x11::x11::xlib::{XFlush, XMoveWindow};
use gtk4::gio::{self};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Builder, Button, EventControllerScroll, EventControllerScrollFlags, Label, Revealer, glib};
use log::{error, info, warn};
use std::cell::{Cell, RefCell};
use std::env;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use shared_structures::{CommandType, SharedCommand, SharedMessage, SharedRingBuffer, TagStatus};
use xbar_core::audio_manager::AudioManager;
use xbar_core::initialize_logging;
use xbar_core::system_monitor::SystemMonitor;

use gtk4::glib::ControlFlow;

static STYLES_APPLIED: OnceLock<()> = OnceLock::new();

fn apply_styles_once() {
    STYLES_APPLIED.get_or_init(|| {
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(include_str!("styles.css"));
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

// ========= 事件与命令 =========
enum AppEvent {
    SharedMessage(SharedMessage),
}

// ========= 常量 =========
const CPU_REDRAW_THRESHOLD: f64 = 0.01; // 1%
const MEM_REDRAW_THRESHOLD: f64 = 0.005; // 0.5%

const METRIC_LEVEL_CLASSES: [&str; 4] = ["level-ok", "level-warn", "level-high", "level-crit"];

// 胶囊颜色阈值（占用比例）
const LEVEL_WARN: f64 = 0.50; // 50%
const LEVEL_HIGH: f64 = 0.75; // 75%
const LEVEL_CRIT: f64 = 0.90; // 90%

// CSS 类 bit 掩码
const CLS_SELECTED: u8 = 1 << 0;
const CLS_OCCUPIED: u8 = 1 << 1;
const CLS_FILLED: u8 = 1 << 2;
const CLS_URGENT: u8 = 1 << 3;
const CLS_EMPTY: u8 = 1 << 4;

// 默认 tag 图标：尽量选单码位/常见字体可用的符号，显示更统一。
// 你可以用 GTK_BAR_TAG_LABELS 覆盖成自己的 9 个图标/字符。
const DEFAULT_TAG_LABELS: [&str; 9] = [
    "🖥", // 1: terminal / system
    "🌐", // 2: web
    "💻", // 3: code
    "💬", // 4: chat
    "📝", // 5: notes
    "🎵", // 6: music
    "🎮", // 7: game
    "⚙", // 8: settings
    "📁", // 9: files
];

// ========= 状态 =========
#[allow(dead_code)]
struct AppState {
    // UI state
    active_tab: usize,
    layout_symbol: String,
    layout_open: bool,
    monitor_num: u8,
    show_seconds: bool,
    tag_status_vec: Vec<TagStatus>,

    // Components
    audio_manager: AudioManager,
    system_monitor: SystemMonitor,

    // Last values to control redraw
    last_cpu_usage: f64,
    last_mem_fraction: f64,

    // 上一帧胶囊的等级（0..=3），255 表示未初始化
    last_cpu_level: u8,
    last_mem_level: u8,

    // 上一帧每个 tab 的 class 掩码，用于差量更新
    last_class_masks: Vec<u8>,

    // 最近消息时间戳
    last_message_ts: u128,

    // 音量 UI 差量更新
    last_volume_device: Option<String>,
    last_volume_percent: i32,
    last_volume_muted: bool,

    // 主题：true=dark, false=light
    theme_dark: bool,
}

impl AppState {
    fn new(theme_dark: bool) -> Self {
        Self {
            active_tab: 0,
            layout_symbol: " ? ".to_string(),
            layout_open: false,
            monitor_num: 0,
            show_seconds: false,
            tag_status_vec: Vec::new(),
            audio_manager: AudioManager::new(),
            system_monitor: SystemMonitor::new(10),
            last_cpu_usage: 0.0,
            last_mem_fraction: 0.0,
            last_cpu_level: 255,
            last_mem_level: 255,
            last_class_masks: Vec::new(),
            last_message_ts: 0,
            last_volume_device: None,
            last_volume_percent: -1,
            last_volume_muted: false,
            theme_dark,
        }
    }
}

type SharedAppState = Rc<RefCell<AppState>>;

// ========= Metric 工具 =========
fn usage_to_level_idx(ratio: f64) -> u8 {
    if ratio >= LEVEL_CRIT {
        3
    } else if ratio >= LEVEL_HIGH {
        2
    } else if ratio >= LEVEL_WARN {
        1
    } else {
        0
    }
}

// 统一更新“胶囊”标签：文本 + 颜色 class
fn set_metric_capsule(label: &Label, title: &str, ratio: f64, prev_level: u8) -> u8 {
    let percent = (ratio * 100.0).round().clamp(0.0, 100.0) as i32;
    label.set_text(&format!("{} {}%", title, percent));

    let new_level = usage_to_level_idx(ratio);
    if prev_level == new_level {
        return new_level;
    }

    if prev_level < 4 {
        label.remove_css_class(METRIC_LEVEL_CLASSES[prev_level as usize]);
    } else {
        // 未初始化或脏状态：兜底清理一次
        for cls in METRIC_LEVEL_CLASSES {
            label.remove_css_class(cls);
        }
    }
    label.add_css_class(METRIC_LEVEL_CLASSES[new_level as usize]);
    new_level
}

// ========= 主体应用 =========
struct TabBarApp {
    // GTK widgets
    builder: Builder,
    window: ApplicationWindow,
    tab_buttons: Vec<Button>,
    time_button: Button,
    monitor_label: Label,
    memory_label: Label,
    cpu_label: Label,

    volume_button: Button,

    theme_toggle: Button,

    // 新增：布局开关 + 展开选项
    layout_toggle: Button,
    layout_revealer: Revealer,
    layout_btn_tiled: Button,
    layout_btn_floating: Button,
    layout_btn_monocle: Button,

    // Shared state
    state: SharedAppState,

    shared_buffer_rc: Option<Arc<SharedRingBuffer>>,

    stop_flag: Arc<AtomicBool>,

    // Cached UI-applied values for diff
    ui_last_monitor_num: Cell<u8>,
}

impl TabBarApp {
    fn new(app: &Application, shared_path: String) -> Rc<Self> {
        // 确保样式尽早应用（另外在 main 的 startup 里也会做一次）
        apply_styles_once();

        // 加载 UI
        let builder = Builder::from_string(include_str!("resources/main_layout.ui"));

        // 主窗口
        let window: ApplicationWindow = builder
            .object("main_window")
            .expect("Failed to get main_window from builder");
        window.set_application(Some(app));

        // 可选：减少动画/过渡以降低 CPU 占用（默认不启用）
        // 用法：GTK_BAR_REDUCE_MOTION=1 nix develop -c cargo run -p gtk_bar -- <shared_path>
        let reduce_motion = env::var("GTK_BAR_REDUCE_MOTION")
            .map(|v| v != "0")
            .unwrap_or(false);
        if reduce_motion {
            window.add_css_class("reduce-motion");
        }

        // 标签按钮
        let mut tab_buttons = Vec::new();
        for i in 0..9 {
            let button_id = format!("tab_button_{}", i);
            let button: Button = builder
                .object(&button_id)
                .unwrap_or_else(|| panic!("Failed to get {} from builder", button_id));
            tab_buttons.push(button);
        }

        // Tag icon/label：默认用一组语义化 icon；可用 GTK_BAR_TAG_LABELS 自定义（逗号分隔）
        // 例：GTK_BAR_TAG_LABELS='🖥,🌐,💻,💬,📝,🎵,🎮,⚙,📁'
        Self::apply_tag_labels(&tab_buttons);

        // 其他组件
        let time_button: Button = builder
            .object("time_label")
            .expect("Failed to get time_label from builder");
        let monitor_label: Label = builder
            .object("monitor_label")
            .expect("Failed to get monitor_label from builder");
        let memory_label: Label = builder
            .object("memory_label")
            .expect("Failed to get memory_label from builder");
        let cpu_label: Label = builder
            .object("cpu_label")
            .expect("Failed to get cpu_label from builder");

        let volume_button: Button = builder
            .object("volume_button")
            .expect("Failed to get volume_button from builder");

        let theme_toggle: Button = builder
            .object("theme_toggle")
            .expect("Failed to get theme_toggle from builder");

        // 布局开关 + 选项
        let layout_toggle: Button = builder
            .object("layout_toggle")
            .expect("Failed to get layout_toggle");
        let layout_revealer: Revealer = builder
            .object("layout_revealer")
            .expect("Failed to get layout_revealer");

        // reduce-motion 时关掉 Revealer 的 slide 动画（避免额外重绘/合成）
        if reduce_motion {
            layout_revealer.set_transition_duration(0);
            layout_revealer.set_transition_type(gtk4::RevealerTransitionType::None);
        }

        // 主题：默认 dark，可用 GTK_BAR_THEME=light|dark 覆盖
        let theme_dark = match env::var("GTK_BAR_THEME").as_deref() {
            Ok("light") => false,
            Ok("dark") => true,
            _ => true,
        };
        window.remove_css_class("theme-dark");
        window.remove_css_class("theme-light");
        window.add_css_class(if theme_dark { "theme-dark" } else { "theme-light" });
        theme_toggle.set_label(if theme_dark { "🌙" } else { "☀" });
        let layout_btn_tiled: Button = builder
            .object("layout_option_tiled")
            .expect("Failed to get layout_option_tiled");
        let layout_btn_floating: Button = builder
            .object("layout_option_floating")
            .expect("Failed to get layout_option_floating");
        let layout_btn_monocle: Button = builder
            .object("layout_option_monocle")
            .expect("Failed to get layout_option_monocle");

        // 状态
        let state: SharedAppState = Rc::new(RefCell::new(AppState::new(theme_dark)));

        // 异步事件通道（worker -> 主线程）
        let (ui_sender, ui_receiver) = async_channel::unbounded::<AppEvent>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let shared_buffer_rc =
            SharedRingBuffer::create_shared_ring_buffer_aux(&shared_path).map(Arc::new);
        let shared_buffer_rc_clone = shared_buffer_rc.clone();
        let stop_flag_clone = stop_flag.clone();
        thread::spawn(move || {
            worker_thread(shared_buffer_rc_clone, ui_sender, stop_flag_clone);
        });

        let app_instance = Rc::new(Self {
            builder,
            window,
            tab_buttons,
            time_button,
            monitor_label,
            memory_label,
            cpu_label,
            volume_button,
            theme_toggle,
            layout_toggle,
            layout_revealer,
            layout_btn_tiled,
            layout_btn_floating,
            layout_btn_monocle,
            state,
            shared_buffer_rc,
            stop_flag,
            ui_last_monitor_num: Cell::new(255),
        });

        // 为 CPU/内存标签添加基础胶囊样式
        app_instance.cpu_label.add_css_class("metric-label");
        app_instance.memory_label.add_css_class("metric-label");

        // 首次音量 UI 同步
        app_instance.update_volume_display();

        // 使用 glib::spawn_future_local 在主线程消费异步通道
        {
            let app_clone = app_instance.clone();
            glib::spawn_future_local(async move {
                while let Ok(event) = ui_receiver.recv().await {
                    match event {
                        AppEvent::SharedMessage(message) => {
                            app_clone.on_shared_message(message);
                        }
                    }
                }
            });
        }

        // 事件绑定
        Self::setup_event_handlers(app_instance.clone());

        // 定时器：每秒更新时间
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(1, move || {
                app_clone.update_time_display();
                ControlFlow::Continue
            });
        }

        // 定时器：每秒刷新音量状态（内部有 500ms 节流）
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(1, move || {
                app_clone.update_volume_display();
                ControlFlow::Continue
            });
        }
        // 定时器：每2秒更新系统资源（含阈值和等级变化检测）
        {
            let app_clone = app_instance.clone();
            glib::timeout_add_seconds_local(2, move || {
                if let Ok(mut st) = app_clone.state.try_borrow_mut() {
                    st.system_monitor.update_if_needed();
                    let Some((memory_available, memory_used, cpu_average)) = st
                        .system_monitor
                        .get_snapshot()
                        .map(|s| (s.memory_available, s.memory_used, s.cpu_average))
                    else {
                        return ControlFlow::Continue;
                    };

                    let total = memory_available + memory_used;
                    if total == 0 {
                        return ControlFlow::Continue;
                    }

                    // 内存占用比例
                    let mem_ratio = (memory_used as f64 / total as f64).clamp(0.0, 1.0);
                    let prev_mem = st.last_mem_fraction;
                    let mem_level_changed = usage_to_level_idx(mem_ratio) != st.last_mem_level;
                    if (mem_ratio - prev_mem).abs() > MEM_REDRAW_THRESHOLD || mem_level_changed {
                        st.last_mem_fraction = mem_ratio;
                        st.last_mem_level = set_metric_capsule(
                            &app_clone.memory_label,
                            "MEM",
                            mem_ratio,
                            st.last_mem_level,
                        );
                    }

                    // CPU 占用比例（0~1）
                    let cpu_ratio = (cpu_average as f64 / 100.0).clamp(0.0, 1.0);
                    let prev_cpu = st.last_cpu_usage;
                    let cpu_level_changed = usage_to_level_idx(cpu_ratio) != st.last_cpu_level;
                    if (cpu_ratio - prev_cpu).abs() > CPU_REDRAW_THRESHOLD || cpu_level_changed {
                        st.last_cpu_usage = cpu_ratio;
                        st.last_cpu_level = set_metric_capsule(
                            &app_clone.cpu_label,
                            "CPU",
                            cpu_ratio,
                            st.last_cpu_level,
                        );
                    }
                }
                ControlFlow::Continue
            });
        }

        // 首次时间显示
        app_instance.update_time_display();
        // 首次布局 UI 同步（默认 closed）
        app_instance.update_layout_ui();
        // 首次 tab 样式同步：让窗口一开始就按最终样式计算尺寸，避免第一次交互时出现高度抖动
        app_instance.update_ui();

        app_instance
    }

    fn apply_tag_labels(tab_buttons: &[Button]) {
        let custom = env::var("GTK_BAR_TAG_LABELS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim())
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        for (idx, b) in tab_buttons.iter().enumerate() {
            if let Some(v) = custom.as_ref().and_then(|v| v.get(idx)) {
                b.set_label(v);
            } else {
                b.set_label(DEFAULT_TAG_LABELS.get(idx).copied().unwrap_or("?"));
            }
        }
    }

    fn setup_event_handlers(app: Rc<Self>) {
        // 标签按钮点击
        for (i, button) in app.tab_buttons.iter().enumerate() {
            button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_tab_selected(app.clone(), i);
                }
            });
        }

        // 布局开关
        app.layout_toggle.connect_clicked({
            let app = app.clone();
            move |_| {
                if let Ok(mut st) = app.state.try_borrow_mut() {
                    st.layout_open = !st.layout_open;
                }
                app.update_layout_ui();
            }
        });

        // 布局选项
        app.layout_btn_tiled.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 0);
            }
        });
        app.layout_btn_floating.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 1);
            }
        });
        app.layout_btn_monocle.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_layout_clicked(app.clone(), 2);
            }
        });

        // 时间按钮
        app.time_button.connect_clicked({
            let app = app.clone();
            move |_| {
                Self::handle_toggle_seconds(app.clone());
            }
        });

        // 主题切换
        app.theme_toggle.connect_clicked({
            let app = app.clone();
            move |_| {
                if let Ok(mut st) = app.state.try_borrow_mut() {
                    st.theme_dark = !st.theme_dark;
                    app.window.remove_css_class("theme-dark");
                    app.window.remove_css_class("theme-light");
                    app.window
                        .add_css_class(if st.theme_dark { "theme-dark" } else { "theme-light" });
                    app.theme_toggle.set_label(if st.theme_dark { "🌙" } else { "☀" });
                }
            }
        });

        // 音量：点击切换静音；滚轮调节音量（默认步进 5%，静音时滚轮会自动取消静音）
        app.volume_button.connect_clicked({
            let app = app.clone();
            move |_| {
                if let Ok(mut st) = app.state.try_borrow_mut() {
                    st.audio_manager.update_if_needed();
                    if let Some(dev) = st.audio_manager.get_master_device().cloned()
                        && dev.has_switch_control
                        && let Err(e) = st.audio_manager.toggle_mute(&dev.name)
                    {
                        warn!("Failed to toggle mute: {e}");
                    }
                }
                app.update_volume_display();
            }
        });

        {
            let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
            let app_for_cb = app.clone();
            scroll.connect_scroll(move |_, dx, dy| {
                // 兼容触控板：优先取绝对值更大的那个方向
                let delta = if dy.abs() >= dx.abs() { dy } else { dx };
                if delta == 0.0 {
                    return glib::Propagation::Proceed;
                }

                let step: i32 = if delta < 0.0 { 5 } else { -5 };
                if let Ok(mut st) = app_for_cb.state.try_borrow_mut() {
                    st.audio_manager.update_if_needed();
                    if let Some(dev) = st.audio_manager.get_master_device().cloned() {
                        let new_volume = (dev.volume + step).clamp(0, 100);
                        let mute = if dev.is_muted { false } else { dev.is_muted };
                        if let Err(e) = st.audio_manager.set_volume(&dev.name, new_volume, mute) {
                            warn!("Failed to set volume: {e}");
                        }
                    }
                }
                app_for_cb.update_volume_display();
                glib::Propagation::Stop
            });
            app.volume_button.add_controller(scroll);
        }

        // 截图按钮
        if let Some(screenshot_button) = app.builder.object::<Button>("screenshot_button") {
            screenshot_button.connect_clicked({
                let app = app.clone();
                move |_| {
                    Self::handle_screenshot(app.clone());
                }
            });
        }
    }

    // ========= Worker事件处理 =========
    fn on_shared_message(&self, message: SharedMessage) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            let ts: u128 = message.timestamp.into();
            if st.last_message_ts == ts {
                return; // 去重
            }
            st.last_message_ts = ts;

            st.layout_symbol = message.monitor_info.get_ltsymbol();
            st.monitor_num = message.monitor_info.monitor_num as u8;
            st.tag_status_vec = message.monitor_info.tag_status_vec.to_vec();

            // 更新活动标签
            for (idx, tag) in message.monitor_info.tag_status_vec.iter().enumerate() {
                if tag.is_selected {
                    st.active_tab = idx;
                    break;
                }
            }

            // 确保掩码数组长度匹配
            if st.last_class_masks.len() != self.tab_buttons.len() {
                st.last_class_masks = vec![0u8; self.tab_buttons.len()];
            }
        }
        // 更新 UI（差量）
        self.update_ui();
        self.update_layout_ui();
    }

    // ========= 交互 =========
    fn handle_tab_selected(app: Rc<Self>, index: usize) {
        info!("Tab selected: {}", index);
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.active_tab = index;
            if let Some(command) = Self::build_tag_command(&st, true)
                && let Some(shared_buffer) = app.shared_buffer_rc.as_ref()
            {
                let _ = shared_buffer.send_command(command);
            }
        }
        app.update_tab_styles();
    }

    fn handle_layout_clicked(app: Rc<Self>, layout_index: u32) {
        if let Ok(st) = app.state.try_borrow() {
            let monitor_id = st.monitor_num as i32;
            let command = SharedCommand::new(CommandType::SetLayout, layout_index, monitor_id);
            if let Some(shared_buffer) = app.shared_buffer_rc.as_ref() {
                let _ = shared_buffer.send_command(command);
            }
            info!("Sent SetLayout command: layout_index={}", layout_index);
        }
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.layout_open = false; // 选择后收起
        }
        app.update_layout_ui();
    }

    fn handle_toggle_seconds(app: Rc<Self>) {
        if let Ok(mut st) = app.state.try_borrow_mut() {
            st.show_seconds = !st.show_seconds;
        }
        app.update_time_display();
    }

    fn handle_screenshot(_app: Rc<Self>) {
        info!("Taking screenshot");
        let _ = std::process::Command::new("flameshot").arg("gui").spawn();
    }

    // ========= UI 更新 =========
    fn update_ui(&self) {
        if let Ok(st) = self.state.try_borrow() {
            // monitor_label 差量
            if self.ui_last_monitor_num.get() != st.monitor_num {
                let monitor_icon = Self::monitor_num_to_icon(st.monitor_num);
                self.monitor_label.set_text(monitor_icon);
                self.ui_last_monitor_num.set(st.monitor_num);
            }
        }
        self.update_tab_styles();
    }

    fn update_tab_styles(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            if st.last_class_masks.len() != self.tab_buttons.len() {
                st.last_class_masks = vec![0u8; self.tab_buttons.len()];
            }

            const CLASS_BITS: &[(u8, &str)] = &[
                (CLS_SELECTED, "selected"),
                (CLS_OCCUPIED, "occupied"),
                (CLS_FILLED, "filled"),
                (CLS_URGENT, "urgent"),
                (CLS_EMPTY, "empty"),
            ];

            for (i, button) in self.tab_buttons.iter().enumerate() {
                let tag_opt = st.tag_status_vec.get(i);
                let desired_mask = Self::classes_mask_for(tag_opt, i == st.active_tab);
                let prev_mask = st.last_class_masks[i];

                if desired_mask == prev_mask {
                    continue;
                }

                let diff = desired_mask ^ prev_mask;
                for (bit, class_name) in CLASS_BITS {
                    if diff & *bit == 0 {
                        continue;
                    }
                    if desired_mask & *bit != 0 {
                        button.add_css_class(class_name);
                    } else {
                        button.remove_css_class(class_name);
                    }
                }

                st.last_class_masks[i] = desired_mask;
            }
        }
    }

    // 新增：布局 UI 更新（切换 open/closed、高亮当前布局、更新 toggle 文本）
    fn update_layout_ui(&self) {
        if let Ok(st) = self.state.try_borrow() {
            // 开关按钮文本：显示当前布局符号
            self.layout_toggle.set_label(&st.layout_symbol);

            // revealer 展开/收起
            self.layout_revealer.set_reveal_child(st.layout_open);

            // 开关按钮 open/closed 类
            self.layout_toggle.remove_css_class("open");
            self.layout_toggle.remove_css_class("closed");
            self.layout_toggle
                .add_css_class(if st.layout_open { "open" } else { "closed" });

            // 当前布局高亮
            let is_tiled = st.layout_symbol.contains("[]=");
            let is_floating = st.layout_symbol.contains("><>");
            let is_monocle = st.layout_symbol.contains("[M]");

            for b in [
                &self.layout_btn_tiled,
                &self.layout_btn_floating,
                &self.layout_btn_monocle,
            ] {
                b.remove_css_class("current");
            }
            if is_tiled {
                self.layout_btn_tiled.add_css_class("current");
            } else if is_floating {
                self.layout_btn_floating.add_css_class("current");
            } else if is_monocle {
                self.layout_btn_monocle.add_css_class("current");
            }
        }
    }

    fn update_time_display(&self) {
        let now = Local::now();
        let show_seconds = if let Ok(st) = self.state.try_borrow() {
            st.show_seconds
        } else {
            false
        };

        let format_str = if show_seconds {
            "%Y-%m-%d %H:%M:%S"
        } else {
            "%Y-%m-%d %H:%M"
        };
        let formatted_time = now.format(format_str).to_string();
        self.time_button.set_label(&formatted_time);
    }

    fn update_volume_display(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            st.audio_manager.update_if_needed();

            let Some(dev) = st.audio_manager.get_master_device().cloned() else {
                if st.last_volume_device.is_some() || st.last_volume_percent != -1 {
                    self.volume_button.set_label("🔇 --%");
                    self.volume_button
                        .set_tooltip_text(Some("No audio device"));
                    self.volume_button.remove_css_class("muted");
                    st.last_volume_device = None;
                    st.last_volume_percent = -1;
                    st.last_volume_muted = false;
                }
                return;
            };

            let vol = dev.volume.clamp(0, 100);
            let muted = dev.is_muted || vol == 0;
            let icon = if muted {
                "🔇"
            } else if vol < 30 {
                "🔈"
            } else if vol < 70 {
                "🔉"
            } else {
                "🔊"
            };

            let should_update = st.last_volume_device.as_deref() != Some(dev.name.as_str())
                || st.last_volume_percent != vol
                || st.last_volume_muted != muted;

            if !should_update {
                return;
            }

            self.volume_button
                .set_label(&format!("{icon} {vol}%"));

            let tooltip = format!(
                "{}: {}%{}",
                dev.description,
                vol,
                if dev.is_muted { " (muted)" } else { "" }
            );
            self.volume_button.set_tooltip_text(Some(&tooltip));

            if dev.is_muted {
                self.volume_button.add_css_class("muted");
            } else {
                self.volume_button.remove_css_class("muted");
            }

            st.last_volume_device = Some(dev.name);
            st.last_volume_percent = vol;
            st.last_volume_muted = muted;
        }
    }

    // ========= 工具 =========
    fn monitor_num_to_icon(monitor_num: u8) -> &'static str {
        match monitor_num {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "🖥",
        }
    }

    fn classes_mask_for(tag: Option<&TagStatus>, is_active_index: bool) -> u8 {
        if let Some(t) = tag {
            if t.is_urg {
                CLS_URGENT
            } else if t.is_filled {
                CLS_FILLED
            } else if t.is_selected && t.is_occ {
                CLS_SELECTED | CLS_OCCUPIED
            } else if t.is_selected || is_active_index {
                CLS_SELECTED
            } else if t.is_occ {
                CLS_OCCUPIED
            } else {
                CLS_EMPTY
            }
        } else if is_active_index {
            CLS_SELECTED
        } else {
            CLS_EMPTY
        }
    }

    fn build_tag_command(state: &AppState, is_view: bool) -> Option<SharedCommand> {
        if state.active_tab >= 32 {
            return None;
        }
        let tag_bit: u32 = 1u32 << (state.active_tab as u32);
        let monitor_id = state.monitor_num as i32;
        let cmd = if is_view {
            SharedCommand::view_tag(tag_bit, monitor_id)
        } else {
            SharedCommand::toggle_tag(tag_bit, monitor_id)
        };
        Some(cmd)
    }

    #[allow(dead_code)]
    #[cfg(all(target_os = "linux", feature = "x11"))]
    fn resize_window_to_monitor(
        &self,
        expected_x: i32,
        expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        self.window
            .set_default_size(expected_width, expected_height);
        if let Some(display) = gtk4::gdk::Display::default() {
            unsafe {
                if let Some(x11_display) = display.downcast_ref::<gdk4_x11::X11Display>() {
                    let xdisplay = x11_display.xdisplay();
                    if let Some(surface) = self.window.surface()
                        && let Some(x11_surface) =
                            surface.downcast_ref::<gdk4_x11::X11Surface>()
                    {
                        let xwindow = x11_surface.xid();
                        XMoveWindow(xdisplay as *mut _, xwindow, expected_x, expected_y);
                        XFlush(xdisplay as *mut _);
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    #[cfg(not(all(target_os = "linux", feature = "x11")))]
    fn resize_window_to_monitor(
        &self,
        _expected_x: i32,
        _expected_y: i32,
        expected_width: i32,
        expected_height: i32,
    ) {
        self.window
            .set_default_size(expected_width, expected_height);
    }

    fn show(&self) {
        self.window.present();
    }
}

// ========= Worker 线程：独占 SharedRingBuffer =========
fn worker_thread(
    shared_buffer_rc: Option<Arc<SharedRingBuffer>>,
    ui_sender: async_channel::Sender<AppEvent>,
    stop_flag: Arc<AtomicBool>,
) {
    if let Some(shared_buffer) = shared_buffer_rc {
        let mut prev_timestamp: u128 = 0;
        while !stop_flag.load(Ordering::Relaxed) {
            match shared_buffer.wait_for_message(Some(Duration::from_millis(500))) {
                Ok(true) => {
                    if let Ok(Some(message)) = shared_buffer.try_read_latest_message() {
                        let ts: u128 = message.timestamp.into();
                        if ts != prev_timestamp {
                            prev_timestamp = ts;
                            if let Err(e) = ui_sender.try_send(AppEvent::SharedMessage(message))
                                && !e.is_full()
                            {
                                warn!("Failed to send SharedMessage to UI: {}", e);
                            }
                        }
                    }
                }
                Ok(false) => {
                    // timeout
                }
                Err(e) => {
                    error!("[worker] wait_for_message failed: {}", e);
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }
    info!("Worker thread exited");
}

// ========= main =========
fn main() -> glib::ExitCode {
    let args: Vec<String> = env::args().collect();
    let shared_path = args.get(1).cloned().unwrap_or_default();

    if let Err(e) = initialize_logging("gtk_bar", &shared_path) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting GTK4 Bar (layout selector optimized like iced_bar)");

    // GTK 应用
    let app = Application::builder()
        .application_id("dev.gtk.bar")
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // 尽早注入 CSS provider，避免窗口先按默认主题分配尺寸，随后再被自定义 CSS 收缩/扩张。
    app.connect_startup(|_| {
        apply_styles_once();
    });

    let shared_path_clone = shared_path.clone();
    app.connect_activate(move |app| {
        let app_instance = TabBarApp::new(app, shared_path_clone.clone());
        app_instance.show();

        // Ensure the worker thread exits quickly on close/shutdown.
        {
            let stop = app_instance.stop_flag.clone();
            app_instance.window.connect_close_request(move |_| {
                stop.store(true, Ordering::Relaxed);
                glib::Propagation::Proceed
            });
        }

        {
            let stop = app_instance.stop_flag.clone();
            app.connect_shutdown(move |_| {
                stop.store(true, Ordering::Relaxed);
            });
        }
    });

    // 文件打开处理
    app.connect_open(move |app, files, hint| {
        info!(
            "App received {} files to open with hint: {}",
            files.len(),
            hint
        );
        for file in files {
            if let Some(path) = file.path() {
                info!("File to open: {:?}", path);
            }
        }
        app.activate();
    });

    // 命令行处理
    app.connect_command_line(move |app, command_line| {
        let args = command_line.arguments();
        info!("Command line arguments: {:?}", args);
        app.activate();
        0.into()
    });

    app.run()
}
