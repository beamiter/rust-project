use gtk4::gdk::ffi::GDK_BUTTON_PRIMARY;
use gtk4::gdk::Key;
use gtk4::gdk::ModifierType;
use gtk4::gdk::RGBA;
use gtk4::gio::{self, Cancellable};
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, Label, Notebook};
use gtk4::{EventControllerKey, GestureClick};
use log::{LevelFilter, Log, Metadata, Record};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use vte4::Format;
use vte4::{CursorBlinkMode, CursorShape, PtyFlags, Terminal};
use vte4::{TerminalExt, TerminalExtManual};

struct SimpleStderrLogger {
    level: LevelFilter,
}

impl Log for SimpleStderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn parse_level_filter(input: &str) -> LevelFilter {
    match input.trim().to_ascii_lowercase().as_str() {
        "off" => LevelFilter::Off,
        "error" => LevelFilter::Error,
        "warn" | "warning" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}

fn init_logging() {
    let level = std::env::var("JTERM4_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .as_deref()
        .map(parse_level_filter)
        .unwrap_or(LevelFilter::Warn);

    let _ = log::set_boxed_logger(Box::new(SimpleStderrLogger { level }));
    log::set_max_level(level);
}

#[derive(Clone)]
struct Config {
    window_opacity: f64,
    terminal_scrollback_lines: u32,
    font_desc: String,
    default_font_scale: f64,
    foreground: RGBA,
    background: RGBA,
    cursor: RGBA,
    cursor_foreground: RGBA,
}

#[derive(Clone)]
struct UiState {
    window: ApplicationWindow,
    notebook: Notebook,
    tab_counter: Rc<Cell<u32>>,
    font_scale: Rc<Cell<f64>>,
    ctrl_clicked: Rc<Cell<bool>>,
    shell_argv: Rc<Vec<String>>,
    config: Rc<Config>,
}

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok())
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok())
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

fn env_rgba(name: &str) -> Option<RGBA> {
    env_string(name).and_then(|v| RGBA::parse(&v).ok())
}

fn load_config() -> Config {
    let default_font_desc = "SauceCodePro Nerd Font Regular 12".to_string();
    let default_foreground = RGBA::parse("#f8f7e9").unwrap();
    let default_background = RGBA::parse("#121616").unwrap();
    let default_cursor = RGBA::parse("#7fb80e").unwrap();
    let default_cursor_foreground = RGBA::parse("#1b315e").unwrap();

    let window_opacity = env_f64("JTERM4_OPACITY").unwrap_or(0.95).clamp(0.01, 1.0);
    let terminal_scrollback_lines = env_u32("JTERM4_SCROLLBACK").unwrap_or(5000);
    let default_font_scale = env_f64("JTERM4_FONT_SCALE").unwrap_or(1.0).clamp(0.1, 10.0);

    let font_desc = env_string("JTERM4_FONT").unwrap_or(default_font_desc);

    let foreground = env_rgba("JTERM4_FG").unwrap_or(default_foreground);
    let background = env_rgba("JTERM4_BG").unwrap_or(default_background);
    let cursor = env_rgba("JTERM4_CURSOR").unwrap_or(default_cursor);
    let cursor_foreground = env_rgba("JTERM4_CURSOR_FG").unwrap_or(default_cursor_foreground);

    Config {
        window_opacity,
        terminal_scrollback_lines,
        font_desc,
        default_font_scale,
        foreground,
        background,
        cursor,
        cursor_foreground,
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn find_executable_in_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|candidate| is_executable(candidate))
}

fn fish_has_working_bass(fish_path: &Path) -> bool {
    // Spawn fish without user config to avoid startup-time side effects.
    // We also *execute* bass once because some bass implementations try to
    // translate bash aliases into fish aliases (which can fail for bash-only
    // alias definitions). If bass can't run, we should not use it.
    Command::new(fish_path)
        .args([
            "--no-config",
            "-c",
            // `type -q` checks autoloaded functions; `bass "true"` validates runtime.
            "type -q bass; and bass \"true\"",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn choose_shell_argv() -> Vec<String> {
    // Prefer fish.
    if let Some(fish_path) = find_executable_in_path("fish") {
        // If bass works, use it to import ~/.bashrc *before* the prompt.
        if fish_has_working_bass(&fish_path) {
            let init_cmd = "if test -f ~/.bashrc; bass source ~/.bashrc; end";
            return vec![
                fish_path.to_string_lossy().to_string(),
                "-l".to_string(),
                "-i".to_string(),
                "-C".to_string(),
                init_cmd.to_string(),
            ];
        }

        // bass missing/broken: still use fish (as requested), just without importing bashrc.
        return vec![
            fish_path.to_string_lossy().to_string(),
            "-l".to_string(),
            "-i".to_string(),
        ];
    }

    if let Some(bash_path) = find_executable_in_path("bash") {
        return vec![bash_path.to_string_lossy().to_string(), "-l".to_string()];
    }

    // Last resort: POSIX sh
    vec!["sh".to_string()]
}

fn create_terminal(config: &Config, font_scale: f64) -> Terminal {
    let terminal = Terminal::builder()
        .hexpand(true)
        .vexpand(true)
        .name("term_name")
        .can_focus(true)
        .allow_hyperlink(true)
        .bold_is_bright(true)
        .input_enabled(true)
        .scrollback_lines(config.terminal_scrollback_lines)
        .cursor_blink_mode(CursorBlinkMode::Off)
        .cursor_shape(CursorShape::Block)
        .font_scale(font_scale)
        .opacity(1.0)
        .pointer_autohide(true)
        .build();

    terminal.set_mouse_autohide(true);

    // Set colors
    let palette: [&RGBA; 16] = [
        &RGBA::parse("#130c0e").unwrap(),
        &RGBA::parse("#ed1941").unwrap(),
        &RGBA::parse("#45b97c").unwrap(),
        &RGBA::parse("#fdb933").unwrap(),
        &RGBA::parse("#2585a6").unwrap(),
        &RGBA::parse("#ae5039").unwrap(),
        &RGBA::parse("#009ad6").unwrap(),
        &RGBA::parse("#fffef9").unwrap(),
        &RGBA::parse("#7c8577").unwrap(),
        &RGBA::parse("#f05b72").unwrap(),
        &RGBA::parse("#84bf96").unwrap(),
        &RGBA::parse("#ffc20e").unwrap(),
        &RGBA::parse("#7bbfea").unwrap(),
        &RGBA::parse("#f58f98").unwrap(),
        &RGBA::parse("#33a3dc").unwrap(),
        &RGBA::parse("#f6f5ec").unwrap(),
    ];
    terminal.set_colors(Some(&config.foreground), Some(&config.background), &palette);
    terminal.set_color_bold(None);
    terminal.set_color_cursor(Some(&config.cursor));
    terminal.set_color_cursor_foreground(Some(&config.cursor_foreground));

    // Set font
    let font_desc = FontDescription::from_string(&config.font_desc);
    terminal.set_font(Some(&font_desc));

    // Set regex for hyperlinks
    let regex_pattern = vte4::Regex::for_match(
        r"[a-z]+://[[:graph:]]+",
        pcre2_sys::PCRE2_CASELESS | pcre2_sys::PCRE2_MULTILINE,
    );
    terminal.match_add_regex(&regex_pattern.unwrap(), 0);

    terminal.connect_bell(move |_| {
        log::debug!("Bell signal received");
    });

    terminal
}

fn terminal_working_directory(terminal: &Terminal) -> Option<String> {
    let uri = terminal.current_directory_uri()?;
    let file = gio::File::for_uri(uri.as_str());
    file.path()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
}

fn tabs_state_file_path() -> PathBuf {
    glib::user_config_dir()
        .join("jterm4")
        .join("tabs.state")
}

fn parse_tabs_state(contents: &str) -> (Option<u32>, Vec<String>) {
    let mut current_page: Option<u32> = None;
    let mut paths: Vec<String> = Vec::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("current_page=") {
            current_page = rest.trim().parse::<u32>().ok();
            continue;
        }
        paths.push(line.to_string());
    }

    (current_page, paths)
}

fn load_tabs_state() -> (Option<u32>, Vec<String>) {
    let path = tabs_state_file_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return (None, Vec::new());
    };
    parse_tabs_state(&contents)
}

fn save_tabs_state(notebook: &Notebook) {
    let path = tabs_state_file_path();
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!("Failed to create state dir {}: {err}", parent.display());
            return;
        }
    }

    let home = std::env::var("HOME").ok();
    let n_pages = notebook.n_pages();
    let mut lines: Vec<String> = Vec::with_capacity((n_pages as usize) + 1);
    if let Some(current) = notebook.current_page() {
        lines.push(format!("current_page={current}"));
    }

    for i in 0..n_pages {
        let Some(widget) = notebook.nth_page(Some(i)) else {
            continue;
        };
        let Ok(terminal) = widget.downcast::<Terminal>() else {
            continue;
        };

        let dir = terminal_working_directory(&terminal)
            .or_else(|| home.clone())
            .unwrap_or_else(|| "/".to_string());
        lines.push(dir);
    }

    let payload = lines.join("\n") + "\n";
    if let Err(err) = fs::write(&path, payload) {
        log::warn!("Failed to write state file {}: {err}", path.display());
    }
}

fn spawn_shell(terminal: &Terminal, argv_owned: &[String], working_directory: Option<&str>) {
    let argv: Vec<&str> = argv_owned.iter().map(|s| s.as_str()).collect();

    // Use empty envv to inherit all environment variables from parent process
    let envv: &[&str] = &[];
    let spawn_flags = SpawnFlags::SEARCH_PATH;
    let cancellable: Option<&Cancellable> = None;
    let home = std::env::var("HOME").ok();
    let working_directory = working_directory.or(home.as_deref());
    terminal.spawn_async(
        PtyFlags::DEFAULT,
        working_directory,
        &argv,
        envv,
        spawn_flags,
        || {},
        -1,
        cancellable,
        |res| log::debug!("spawn_async: {res:?}"),
    );
}

fn open_uri(uri: &str) {
    if let Err(err) = gio::AppInfo::launch_default_for_uri(uri, None::<&gio::AppLaunchContext>) {
        log::warn!("Failed to open URI {uri}: {err}");
    }
}

fn setup_terminal_click_handler(terminal: &Terminal, ctrl_clicked: Rc<Cell<bool>>) {
    let click_controller = GestureClick::new();
    click_controller.set_button(0);
    let terminal_clone = terminal.clone();
    let ctrl_clicked_clone = ctrl_clicked.clone();

    click_controller.connect_pressed(move |controller, n_press, x, y| {
        if n_press == 1 {
            let button = controller.current_button();
            if button == GDK_BUTTON_PRIMARY as u32 {
                let tmp = terminal_clone.check_match_at(x, y);
                if let Some(hyper_link) = tmp.0 {
                    if ctrl_clicked_clone.get() {
                        open_uri(&hyper_link);
                    }
                }
            }
        }
    });

    terminal.add_controller(click_controller);
}

impl UiState {
    fn add_new_tab(&self, working_directory: Option<String>) -> Terminal {
        let tab_num = self.tab_counter.get();
        self.tab_counter.set(tab_num + 1);

        let terminal = create_terminal(&self.config, self.font_scale.get());

        // Setup click handler for hyperlinks
        setup_terminal_click_handler(&terminal, self.ctrl_clicked.clone());

        // Connect child-exited to close the tab
        let notebook_clone = self.notebook.clone();
        let terminal_clone = terminal.clone();
        let window_clone = self.window.clone();
        terminal.connect_child_exited(move |_, _| {
            // Find and remove this terminal's page
            let n_pages = notebook_clone.n_pages();
            for i in 0..n_pages {
                if let Some(page) = notebook_clone.nth_page(Some(i)) {
                    if page == terminal_clone.clone().upcast::<gtk4::Widget>() {
                        notebook_clone.remove_page(Some(i));
                        break;
                    }
                }
            }
            // If no more tabs, close window; otherwise focus new current terminal
            if notebook_clone.n_pages() == 0 {
                window_clone.destroy();
            } else if let Some(new_page) = notebook_clone.current_page() {
                if let Some(widget) = notebook_clone.nth_page(Some(new_page)) {
                    if let Ok(term) = widget.downcast::<Terminal>() {
                        term.grab_focus();
                    }
                }
            }
        });

        // Spawn shell
        spawn_shell(
            &terminal,
            self.shell_argv.as_ref(),
            working_directory.as_deref(),
        );

        // Create tab header with a close button
        let tab_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let label = Label::new(Some(&format!("Terminal {}", tab_num + 1)));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        // Make tabs wider by default so the title is visible.
        // These are character-based hints; the notebook may still shrink tabs when crowded.
        label.set_width_chars(24);
        label.set_max_width_chars(64);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let close_button = gtk4::Button::from_icon_name("window-close-symbolic");
        close_button.set_focus_on_click(false);
        close_button.set_can_focus(false);
        close_button.set_has_frame(false);
        close_button.add_css_class("flat");
        close_button.set_tooltip_text(Some("Close tab"));

        tab_box.append(&label);
        tab_box.append(&close_button);

        let notebook_for_close = self.notebook.clone();
        let window_for_close = self.window.clone();
        let terminal_widget_for_close = terminal.clone().upcast::<gtk4::Widget>();
        close_button.connect_clicked(move |_| {
            let n_pages = notebook_for_close.n_pages();
            for i in 0..n_pages {
                if let Some(page) = notebook_for_close.nth_page(Some(i)) {
                    if page == terminal_widget_for_close {
                        notebook_for_close.remove_page(Some(i));
                        break;
                    }
                }
            }

            if notebook_for_close.n_pages() == 0 {
                window_for_close.destroy();
            } else if let Some(new_page) = notebook_for_close.current_page() {
                if let Some(widget) = notebook_for_close.nth_page(Some(new_page)) {
                    if let Ok(term) = widget.downcast::<Terminal>() {
                        term.grab_focus();
                    }
                }
            }
        });

        // Add to notebook
        let page_num = self.notebook.append_page(&terminal, Some(&tab_box));
        self.notebook.set_tab_reorderable(&terminal, true);
        self.notebook.set_current_page(Some(page_num));

        // Focus the new terminal
        terminal.grab_focus();

        terminal
    }
}

fn main() -> glib::ExitCode {
    init_logging();

    // Shell selection is handled per-terminal spawn:
    // - prefer fish if available
    // - if bass works, import ~/.bashrc before showing the prompt
    // - otherwise fall back to plain fish, and if fish is missing then bash

    let app = Application::builder().application_id("app.jterm4").build();

    app.connect_activate(|app| {
        let config = Rc::new(load_config());

        // Cache shell selection once to avoid extra process probes per new tab.
        let shell_argv = Rc::new(choose_shell_argv());

        let window_opacity = Rc::new(Cell::new(config.window_opacity));
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(800)
            .default_height(600)
            .title("jterm4")
            .name("win_name")
            .opacity(window_opacity.get())
            .build();

        // Create notebook for tabs
        let notebook = Notebook::builder()
            .hexpand(true)
            .vexpand(true)
            .scrollable(true)
            .show_border(false)
            .build();

        // Shared state
        let font_scale = Rc::new(Cell::new(config.default_font_scale));
        let tab_counter = Rc::new(Cell::new(0));
        let ctrl_clicked = Rc::new(Cell::new(false));

        let ui = Rc::new(UiState {
            window: window.clone(),
            notebook: notebook.clone(),
            tab_counter: tab_counter.clone(),
            font_scale: font_scale.clone(),
            ctrl_clicked: ctrl_clicked.clone(),
            shell_argv: shell_argv.clone(),
            config: config.clone(),
        });

        // Restore tabs from last session
        let (saved_current, saved_paths) = load_tabs_state();
        if saved_paths.is_empty() {
            ui.add_new_tab(None);
        } else {
            for p in saved_paths {
                let dir = if Path::new(&p).is_dir() { Some(p) } else { None };
                ui.add_new_tab(dir);
            }

            if let Some(page) = saved_current {
                let n_pages = notebook.n_pages();
                if n_pages > 0 {
                    notebook.set_current_page(Some(page.min(n_pages.saturating_sub(1))));
                }
            }
        }

        // Setup key controller on window level with Capture phase
        // This allows us to intercept shortcuts before the terminal processes them
        let key_controller = EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let font_step = 0.025;
        let opacity_step = 0.025;

        let notebook_clone = notebook.clone();
        let window_clone = window.clone();
        let font_scale_clone = font_scale.clone();
        let ctrl_clicked_clone = ctrl_clicked.clone();
        let window_opacity_clone = window_opacity.clone();
        let ui_clone = ui.clone();

        key_controller.connect_key_pressed(move |_controller, keyval, _keycode, state| {
            // Only log for shortcut keys, not every keypress (to avoid IME interference)
            // println!("connect_key_pressed state:{:?}, keyval: {}", state, keyval);

            // Get current terminal
            let current_page = notebook_clone.current_page();
            let current_terminal = current_page.and_then(|page_num| {
                notebook_clone
                    .nth_page(Some(page_num))
                    .and_then(|widget| widget.downcast::<Terminal>().ok())
            });

            if state.contains(ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK) {
                log::debug!(
                    "Ctrl+Shift shortcut: {} ({})",
                    keyval,
                    keyval.name().unwrap_or_default()
                );
                match keyval {
                    Key::T | Key::t => {
                        // New tab
                        log::info!("New tab");
                        let working_directory = current_terminal
                            .as_ref()
                            .and_then(terminal_working_directory);
                        ui_clone.add_new_tab(working_directory);
                        return true.into();
                    }
                    Key::W | Key::w => {
                        // Close current tab
                        log::info!("Close tab");
                        if let Some(page_num) = notebook_clone.current_page() {
                            notebook_clone.remove_page(Some(page_num));
                            if notebook_clone.n_pages() == 0 {
                                window_clone.destroy();
                            } else {
                                // Focus the new current terminal
                                if let Some(new_page) = notebook_clone.current_page() {
                                    if let Some(widget) = notebook_clone.nth_page(Some(new_page)) {
                                        if let Ok(term) = widget.downcast::<Terminal>() {
                                            term.grab_focus();
                                        }
                                    }
                                }
                            }
                        }
                        return true.into();
                    }
                    Key::C | Key::c => {
                        log::debug!("Copy");
                        if let Some(ref term) = current_terminal {
                            term.copy_clipboard_format(Format::Text);
                        }
                        return true.into();
                    }
                    Key::V | Key::v => {
                        log::debug!("Paste");
                        if let Some(ref term) = current_terminal {
                            term.paste_clipboard();
                        }
                        return true.into();
                    }
                    Key::plus => {
                        log::debug!("Font increase");
                        font_scale_clone.set((font_scale_clone.get() + font_step).min(10.0));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::I | Key::i => {
                        log::debug!("Font decrease");
                        font_scale_clone.set((font_scale_clone.get() - font_step).max(0.1));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::O | Key::o => {
                        log::debug!("Font increase");
                        font_scale_clone.set((font_scale_clone.get() + font_step).min(10.0));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::J | Key::j => {
                        log::debug!("Opacity decrease");
                        window_opacity_clone
                            .set((window_opacity_clone.get() - opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity_clone.get());
                        return true.into();
                    }
                    Key::K | Key::k => {
                        log::debug!("Opacity increase");
                        window_opacity_clone
                            .set((window_opacity_clone.get() + opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity_clone.get());
                        return true.into();
                    }
                    Key::Page_Up => {
                        // Previous tab
                        log::debug!("Previous tab");
                        if let Some(page_num) = notebook_clone.current_page() {
                            if page_num > 0 {
                                notebook_clone.set_current_page(Some(page_num - 1));
                            } else {
                                // Wrap to last tab
                                let last = notebook_clone.n_pages().saturating_sub(1);
                                notebook_clone.set_current_page(Some(last));
                            }
                        }
                        return true.into();
                    }
                    Key::Page_Down => {
                        // Next tab
                        log::debug!("Next tab");
                        if let Some(page_num) = notebook_clone.current_page() {
                            let n_pages = notebook_clone.n_pages();
                            if page_num < n_pages - 1 {
                                notebook_clone.set_current_page(Some(page_num + 1));
                            } else {
                                // Wrap to first tab
                                notebook_clone.set_current_page(Some(0));
                            }
                        }
                        return true.into();
                    }
                    _ => {}
                }
            }

            if state.contains(ModifierType::CONTROL_MASK) && !state.contains(ModifierType::SHIFT_MASK) {
                match keyval {
                    Key::minus => {
                        log::debug!("Font decrease");
                        font_scale_clone.set((font_scale_clone.get() - font_step).max(0.1));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::Page_Up => {
                        // Previous tab (Ctrl+Page_Up)
                        log::debug!("Previous tab");
                        if let Some(page_num) = notebook_clone.current_page() {
                            if page_num > 0 {
                                notebook_clone.set_current_page(Some(page_num - 1));
                            } else {
                                let last = notebook_clone.n_pages().saturating_sub(1);
                                notebook_clone.set_current_page(Some(last));
                            }
                        }
                        return true.into();
                    }
                    Key::Page_Down => {
                        // Next tab (Ctrl+Page_Down)
                        log::debug!("Next tab");
                        if let Some(page_num) = notebook_clone.current_page() {
                            let n_pages = notebook_clone.n_pages();
                            if page_num < n_pages - 1 {
                                notebook_clone.set_current_page(Some(page_num + 1));
                            } else {
                                notebook_clone.set_current_page(Some(0));
                            }
                        }
                        return true.into();
                    }
                    _ => {}
                }
            }

            if keyval == Key::Control_L || keyval == Key::Control_R {
                ctrl_clicked_clone.set(true);
                log::trace!("ctrl pressed");
            }

            false.into()
        });

        let ctrl_clicked_clone2 = ctrl_clicked.clone();
        key_controller.connect_key_released(move |_controller, keyval, _keycode, _state| {
            if keyval == Key::Control_L || keyval == Key::Control_R {
                log::trace!("ctrl released");
                ctrl_clicked_clone2.set(false);
            }
        });

        // Focus terminal when switching tabs
        notebook.connect_switch_page(move |_, widget, _page_num| {
            if let Ok(terminal) = widget.clone().downcast::<Terminal>() {
                terminal.grab_focus();
            }
        });

        window.add_controller(key_controller);

        let app_clone = app.clone();
        let notebook_for_save = notebook.clone();
        window.connect_destroy(move |_| {
            save_tabs_state(&notebook_for_save);
            app_clone.quit();
        });

        window.set_child(Some(&notebook));
        window.show();

        // Focus the active terminal after window is shown
        if let Some(page_num) = notebook.current_page() {
            if let Some(widget) = notebook.nth_page(Some(page_num)) {
                if let Ok(terminal) = widget.downcast::<Terminal>() {
                    terminal.grab_focus();
                }
            }
        }
    });

    app.run()
}
