use gtk4::gdk::ffi::GDK_BUTTON_PRIMARY;
use gtk4::gdk::Key;
use gtk4::gdk::ModifierType;
use gtk4::gdk::RGBA;
use gtk4::gio::Cancellable;
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, Label, Notebook};
use gtk4::{EventControllerKey, GestureClick};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use vte4::Format;
use vte4::{CursorBlinkMode, CursorShape, PtyFlags, Terminal};
use vte4::{TerminalExt, TerminalExtManual};

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

fn create_terminal(font_scale: f64) -> Terminal {
    let terminal = Terminal::builder()
        .hexpand(true)
        .vexpand(true)
        .name("term_name")
        .can_focus(true)
        .allow_hyperlink(true)
        .bold_is_bright(true)
        .input_enabled(true)
        .scrollback_lines(5000)
        .cursor_blink_mode(CursorBlinkMode::Off)
        .cursor_shape(CursorShape::Block)
        .font_scale(font_scale)
        .opacity(1.0)
        .pointer_autohide(true)
        .build();

    terminal.set_mouse_autohide(true);

    // Set colors
    let foreground = RGBA::parse("#f8f7e9").unwrap();
    let background = RGBA::parse("#121616").unwrap();
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
    terminal.set_colors(Some(&foreground), Some(&background), &palette);
    terminal.set_color_bold(None);
    terminal.set_color_cursor(Some(&RGBA::parse("#7fb80e").unwrap()));
    terminal.set_color_cursor_foreground(Some(&RGBA::parse("#1b315e").unwrap()));

    // Set font
    let font_desc = FontDescription::from_string("SauceCodePro Nerd Font Regular 12");
    terminal.set_font(Some(&font_desc));

    // Set regex for hyperlinks
    let regex_pattern = vte4::Regex::for_match(
        r"[a-z]+://[[:graph:]]+",
        pcre2_sys::PCRE2_CASELESS | pcre2_sys::PCRE2_MULTILINE,
    );
    terminal.match_add_regex(&regex_pattern.unwrap(), 0);

    terminal.connect_bell(move |_| {
        println!("Bell signal received");
    });

    terminal
}

fn spawn_shell(terminal: &Terminal) {
    let argv_owned = choose_shell_argv();
    let argv: Vec<&str> = argv_owned.iter().map(|s| s.as_str()).collect();

    // Use empty envv to inherit all environment variables from parent process
    let envv: &[&str] = &[];
    let spawn_flags = SpawnFlags::SEARCH_PATH;
    let cancellable: Option<&Cancellable> = None;
    let home = std::env::var("HOME").ok();
    let working_directory = home.as_deref();
    terminal.spawn_async(
        PtyFlags::DEFAULT,
        working_directory,
        &argv,
        envv,
        spawn_flags,
        || {},
        -1,
        cancellable,
        |res| println!("{:?}", res),
    );
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
                        println!("hyper_link: {}", hyper_link);
                        std::process::Command::new("xdg-open")
                            .arg(hyper_link)
                            .spawn()
                            .expect("Failed to open URL");
                    }
                }
            }
        }
    });

    terminal.add_controller(click_controller);
}

fn add_new_tab(
    notebook: &Notebook,
    tab_counter: Rc<Cell<u32>>,
    window: &ApplicationWindow,
    font_scale: Rc<Cell<f64>>,
    ctrl_clicked: Rc<Cell<bool>>,
) -> Terminal {
    let tab_num = tab_counter.get();
    tab_counter.set(tab_num + 1);

    let terminal = create_terminal(font_scale.get());

    // Setup click handler for hyperlinks
    setup_terminal_click_handler(&terminal, ctrl_clicked);

    // Connect child-exited to close the tab
    let notebook_clone = notebook.clone();
    let terminal_clone = terminal.clone();
    let window_clone = window.clone();
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
    spawn_shell(&terminal);

    // Create tab label
    let label = Label::new(Some(&format!("Terminal {}", tab_num + 1)));

    // Add to notebook
    let page_num = notebook.append_page(&terminal, Some(&label));
    notebook.set_tab_reorderable(&terminal, true);
    notebook.set_current_page(Some(page_num));

    // Focus the new terminal
    terminal.grab_focus();

    terminal
}

fn main() -> glib::ExitCode {
    // Shell selection is handled per-terminal spawn:
    // - prefer fish if available
    // - if bass works, import ~/.bashrc before showing the prompt
    // - otherwise fall back to plain fish, and if fish is missing then bash

    let app = Application::builder().application_id("app.jterm4").build();

    app.connect_activate(|app| {
        let window_opacity = Rc::new(Cell::new(0.95));
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
        let font_scale = Rc::new(Cell::new(1.0));
        let tab_counter = Rc::new(Cell::new(0));
        let ctrl_clicked = Rc::new(Cell::new(false));
        let terminals: Rc<RefCell<Vec<Terminal>>> = Rc::new(RefCell::new(Vec::new()));

        // Add first tab
        let first_terminal = add_new_tab(
            &notebook,
            tab_counter.clone(),
            &window,
            font_scale.clone(),
            ctrl_clicked.clone(),
        );
        terminals.borrow_mut().push(first_terminal.clone());

        // Setup key controller on window level with Capture phase
        // This allows us to intercept shortcuts before the terminal processes them
        let key_controller = EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let font_step = 0.025;
        let opacity_step = 0.025;

        let notebook_clone = notebook.clone();
        let window_clone = window.clone();
        let font_scale_clone = font_scale.clone();
        let tab_counter_clone = tab_counter.clone();
        let ctrl_clicked_clone = ctrl_clicked.clone();
        let window_opacity_clone = window_opacity.clone();
        let terminals_clone = terminals.clone();

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
                println!("Ctrl+Shift detected, keyval: {} ({})", keyval, keyval.name().unwrap_or_default());
                match keyval {
                    Key::T | Key::t => {
                        // New tab
                        println!("New tab: T");
                        let new_terminal = add_new_tab(
                            &notebook_clone,
                            tab_counter_clone.clone(),
                            &window_clone,
                            font_scale_clone.clone(),
                            ctrl_clicked_clone.clone(),
                        );
                        terminals_clone.borrow_mut().push(new_terminal);
                        return true.into();
                    }
                    Key::W | Key::w => {
                        // Close current tab
                        println!("Close tab: W");
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
                        println!("Copy: C");
                        if let Some(ref term) = current_terminal {
                            term.copy_clipboard_format(Format::Text);
                        }
                        return true.into();
                    }
                    Key::V | Key::v => {
                        println!("Paste: V");
                        if let Some(ref term) = current_terminal {
                            term.paste_clipboard();
                        }
                        return true.into();
                    }
                    Key::plus => {
                        println!("Font increase: plus");
                        font_scale_clone.set((font_scale_clone.get() + font_step).min(10.0));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::I | Key::i => {
                        println!("Font decrease: I");
                        font_scale_clone.set((font_scale_clone.get() - font_step).max(0.1));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::O | Key::o => {
                        println!("Font increase: O");
                        font_scale_clone.set((font_scale_clone.get() + font_step).min(10.0));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::J | Key::j => {
                        println!("Opacity decrease: J");
                        window_opacity_clone
                            .set((window_opacity_clone.get() - opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity_clone.get());
                        return true.into();
                    }
                    Key::K | Key::k => {
                        println!("Opacity increase: K");
                        window_opacity_clone
                            .set((window_opacity_clone.get() + opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity_clone.get());
                        return true.into();
                    }
                    Key::Page_Up => {
                        // Previous tab
                        println!("Previous tab: Page_Up");
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
                        println!("Next tab: Page_Down");
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
                        println!("Font decrease: minus");
                        font_scale_clone.set((font_scale_clone.get() - font_step).max(0.1));
                        if let Some(ref term) = current_terminal {
                            term.set_font_scale(font_scale_clone.get());
                        }
                        return true.into();
                    }
                    Key::Page_Up => {
                        // Previous tab (Ctrl+Page_Up)
                        println!("Previous tab: Ctrl+Page_Up");
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
                        println!("Next tab: Ctrl+Page_Down");
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
                println!("ctrl clicked");
            }

            false.into()
        });

        let ctrl_clicked_clone2 = ctrl_clicked.clone();
        key_controller.connect_key_released(move |_controller, keyval, _keycode, _state| {
            if keyval == Key::Control_L || keyval == Key::Control_R {
                println!("ctrl not clicked");
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
        window.connect_destroy(move |_| {
            app_clone.quit();
        });

        window.set_child(Some(&notebook));
        window.show();

        // Focus the terminal after window is shown
        first_terminal.grab_focus();
    });

    app.run()
}
