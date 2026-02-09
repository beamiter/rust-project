use gtk4::gdk::ffi::GDK_BUTTON_PRIMARY;
use gtk4::gdk::Key;
use gtk4::gdk::ModifierType;
use gtk4::gdk::RGBA;
use gtk4::gio::{self, Cancellable};
use gtk4::gio::prelude::FileExt as GioFileExt;
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, Dialog, Entry, Label, Notebook, ResponseType};
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

fn escape_tab_state(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape_tab_state(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek().copied() {
                Some('t') => {
                    out.push('\t');
                    chars.next();
                }
                Some('n') => {
                    out.push('\n');
                    chars.next();
                }
                Some('\\') => {
                    out.push('\\');
                    chars.next();
                }
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_tabs_state(contents: &str) -> (Option<u32>, Vec<(Option<String>, String)>) {
    let mut current_page: Option<u32> = None;
    let mut tabs: Vec<(Option<String>, String)> = Vec::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("current_page=") {
            current_page = rest.trim().parse::<u32>().ok();
            continue;
        }
        if let Some(rest) = line.strip_prefix("tab=") {
            if let Some((name_raw, dir_raw)) = rest.split_once('\t') {
                let name = unescape_tab_state(name_raw);
                let dir = unescape_tab_state(dir_raw);
                tabs.push((Some(name), dir));
            } else {
                let dir = unescape_tab_state(rest);
                tabs.push((None, dir));
            }
            continue;
        }
        tabs.push((None, line.to_string()));
    }

    (current_page, tabs)
}

fn load_tabs_state() -> (Option<u32>, Vec<(Option<String>, String)>) {
    let path = tabs_state_file_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return (None, Vec::new());
    };

    // Consume-on-start: delete after read so only one instance restores this snapshot.
    // Each instance writes its own state on close; the last one closed wins.
    if let Err(err) = fs::remove_file(&path) {
        log::debug!("Failed to remove tabs state {}: {err}", path.display());
    }

    parse_tabs_state(&contents)
}

fn tab_label_text(notebook: &Notebook, widget: &gtk4::Widget) -> Option<String> {
    let tab_label = notebook.tab_label(widget)?;
    let tab_box = tab_label.downcast::<gtk4::Box>().ok()?;
    let first_child = tab_box.first_child()?;
    let label = first_child.downcast::<Label>().ok()?;
    Some(label.text().to_string())
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
        let label_text = tab_label_text(notebook, &terminal.upcast::<gtk4::Widget>())
            .unwrap_or_else(|| format!("Terminal {}", i + 1));
        let line = format!(
            "tab={}\t{}",
            escape_tab_state(&label_text),
            escape_tab_state(&dir)
        );
        lines.push(line);
    }

    let payload = lines.join("\n") + "\n";

    // Write atomically to avoid partially-written state when the process is interrupted.
    let tmp_path = path.with_file_name(
        path.file_name()
            .and_then(|s| s.to_str())
            .map(|name| format!("{name}.tmp"))
            .unwrap_or_else(|| "tabs.state.tmp".to_string()),
    );

    if let Err(err) = fs::write(&tmp_path, &payload) {
        log::warn!(
            "Failed to write temp state file {}: {err}",
            tmp_path.display()
        );
        return;
    }

    if let Err(err) = fs::rename(&tmp_path, &path) {
        // On some platforms rename may fail if the destination exists; fall back to remove+rename.
        let _ = fs::remove_file(&path);
        if let Err(err2) = fs::rename(&tmp_path, &path) {
            log::warn!(
                "Failed to move temp state file {} into place {}: {err} / {err2}",
                tmp_path.display(),
                path.display()
            );
            let _ = fs::remove_file(&tmp_path);
        }
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

fn show_rename_dialog(window: &ApplicationWindow, label: &Label, custom_title: Rc<Cell<bool>>) {
    let dialog = Dialog::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename tab")
        .build();
    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("Rename", ResponseType::Accept);
    dialog.set_default_response(ResponseType::Accept);

    let entry = Entry::new();
    entry.set_text(&label.text());
    entry.set_activates_default(true);
    dialog.content_area().append(&entry);

    let label_clone = label.clone();
    let custom_title_clone = custom_title.clone();
    let value = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            let text = value.text();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                label_clone.set_text(trimmed);
                custom_title_clone.set(true);
            }
        }
        dialog.close();
    });

    dialog.show();
    entry.grab_focus();
}

fn default_tab_title(tab_index_1based: u32, working_directory: Option<&str>) -> String {
    let mut resolved_dir = working_directory.filter(|s| !s.trim().is_empty()).map(|s| s.to_string());

    // If no directory is known (e.g. first launch), default to HOME so the tab has a meaningful title.
    if resolved_dir.is_none() {
        resolved_dir = std::env::var("HOME").ok();
    }

    let Some(dir) = resolved_dir.as_deref() else {
        return format!("Terminal {tab_index_1based}");
    };

    // Normalize trailing slashes.
    let mut normalized = dir.trim_end_matches('/');
    if normalized.is_empty() {
        normalized = "/";
    }

    // Shorten $HOME to ~.
    let home = std::env::var("HOME").ok();
    let display_dir = if let Some(home) = home.as_deref() {
        if normalized == home {
            "~".to_string()
        } else if let Some(rest) = normalized.strip_prefix(home) {
            if rest.starts_with('/') {
                format!("~{rest}")
            } else {
                normalized.to_string()
            }
        } else {
            normalized.to_string()
        }
    } else {
        normalized.to_string()
    };

    if display_dir == "/" || display_dir == "~" {
        return display_dir;
    }

    // Fish-like prompt_pwd: abbreviate intermediate components, keep the last component.
    // Example: /usr/local/bin -> /u/l/bin, ~/projects/rust-project/jwm -> ~/p/r/jwm
    fn shorten_component(component: &str) -> String {
        if component.is_empty() {
            return String::new();
        }
        if component == "." || component == ".." {
            return component.to_string();
        }

        let mut chars = component.chars();
        let first = chars.next().unwrap();
        if first == '.' {
            // Better readability for dot-dirs: ".config" -> ".c".
            if let Some(second) = chars.next() {
                let mut out = String::new();
                out.push(first);
                out.push(second);
                out
            } else {
                ".".to_string()
            }
        } else {
            first.to_string()
        }
    }

    let (prefix, rest) = if let Some(r) = display_dir.strip_prefix("~/") {
        ("~/", r)
    } else if let Some(r) = display_dir.strip_prefix('/') {
        ("/", r)
    } else {
        ("", display_dir.as_str())
    };

    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 1 {
        return format!("{prefix}{rest}");
    }

    let mut out_parts: Vec<String> = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            out_parts.push((*part).to_string());
        } else {
            out_parts.push(shorten_component(part));
        }
    }

    format!("{prefix}{}", out_parts.join("/"))
}

fn looks_like_legacy_default_title(title: &str) -> bool {
    let trimmed = title.trim();
    let Some(rest) = trimmed.strip_prefix("Terminal ") else {
        return false;
    };
    rest.trim().parse::<u32>().is_ok()
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
    fn add_new_tab(&self, working_directory: Option<String>, tab_name: Option<String>) -> Terminal {
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
        let computed_default_title = default_tab_title(tab_num + 1, working_directory.as_deref());
        let (label_text, is_custom) = match tab_name {
            Some(name) => {
                // Treat as non-custom if it matches the computed default title.
                let custom = name != computed_default_title;
                (name, custom)
            }
            None => (computed_default_title, false),
        };
        let label = Label::new(Some(&label_text));
        let custom_title = Rc::new(Cell::new(is_custom));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        // Make tabs wider by default so the title is visible.
        // These are character-based hints; the notebook may still shrink tabs when crowded.
        label.set_width_chars(24);
        label.set_max_width_chars(64);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let rename_click = GestureClick::new();
        rename_click.set_button(GDK_BUTTON_PRIMARY as u32);
        let label_for_rename = label.clone();
        let window_for_rename = self.window.clone();
        let custom_title_for_rename = custom_title.clone();
        rename_click.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                show_rename_dialog(&window_for_rename, &label_for_rename, custom_title_for_rename.clone());
            }
        });
        label.add_controller(rename_click);

        // Auto-update tab title when PWD changes (unless user manually renamed it).
        let label_for_pwd = label.clone();
        let custom_title_for_pwd = custom_title.clone();
        let tab_index_for_pwd = tab_num + 1;
        terminal.connect_notify_local(Some("current-directory-uri"), move |term, _| {
            if custom_title_for_pwd.get() {
                return;
            }
            let Some(dir) = terminal_working_directory(term) else {
                return;
            };
            let new_title = default_tab_title(tab_index_for_pwd, Some(&dir));
            if label_for_pwd.text().as_str() != new_title {
                label_for_pwd.set_text(&new_title);
            }
        });

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

        // Add to notebook right after the current tab when possible.
        let page_num = if let Some(current_page) = self.notebook.current_page() {
            self.notebook
                .insert_page(&terminal, Some(&tab_box), Some(current_page + 1))
        } else {
            self.notebook.append_page(&terminal, Some(&tab_box))
        };
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

        // Restore tabs from last session snapshot (and delete it immediately).
        // Each instance saves its own state on close; the last one closed wins.
        let (saved_current, saved_tabs) = load_tabs_state();
        if saved_tabs.is_empty() {
            ui.add_new_tab(None, None);
        } else {
            for (name, path) in saved_tabs {
                let dir = if Path::new(&path).is_dir() { Some(path) } else { None };
                let effective_name = if dir.is_some() {
                    name.and_then(|n| if looks_like_legacy_default_title(&n) { None } else { Some(n) })
                } else {
                    name
                };
                ui.add_new_tab(dir, effective_name);
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
                        ui_clone.add_new_tab(working_directory, None);
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

        // Save state *before* GTK starts destroying widgets.
        let notebook_for_close_request = notebook.clone();
        window.connect_close_request(move |_| {
            save_tabs_state(&notebook_for_close_request);
            false.into()
        });

        let app_clone = app.clone();
        window.connect_destroy(move |_| {
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
