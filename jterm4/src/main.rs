use gtk4::gdk::ffi::GDK_BUTTON_PRIMARY;
use gtk4::gdk::Key;
use gtk4::gdk::ModifierType;
use gtk4::gdk::RGBA;
use gtk4::gio::Cancellable;
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow};
use gtk4::{EventControllerKey, GestureClick};
use std::cell::Cell;
use std::rc::Rc;
use vte4::Format;
use vte4::{CursorBlinkMode, CursorShape, PtyFlags, Terminal};
use vte4::{TerminalExt, TerminalExtManual};

fn main() -> glib::ExitCode {
    // Create a new GTK application
    let app = Application::builder().application_id("app.jterm4").build();

    // Connect to the "activate" signal of the application
    app.connect_activate(|app| {
        // Create a new application window
        let window_opacity = Cell::new(0.95);
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(320)
            .default_height(200)
            .title("jterm4")
            .name("win_name")
            .opacity(window_opacity.get())
            .build();

        // Create a new VTE terminal widget
        let terminal_font_scale = Cell::new(1.0);
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
            .scrollback_lines(5000)
            .font_scale(terminal_font_scale.get())
            .opacity(1.0)
            .pointer_autohide(true)
            .build();
        terminal.set_mouse_autohide(true);
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

        // Create a new FontDescription
        let font_desc = FontDescription::from_string("SauceCodePro Nerd Font Regular 12");
        // Set the terminal's font
        terminal.set_font(Some(&font_desc));

        let regex_pattern = vte4::Regex::for_match(
            r"[a-z]+://[[:graph:]]+",
            pcre2_sys::PCRE2_CASELESS | pcre2_sys::PCRE2_MULTILINE,
        );
        // let regex_pattern = vte::Regex::new(r"https?://[^\s]+", 0);
        terminal.match_add_regex(&regex_pattern.unwrap(), 0);

        terminal.connect_bell(move |_| {
            println!("Bell signal received");
        });

        let window_clone0 = window.clone();
        terminal.connect_child_exited(move |_, _| {
            window_clone0.destroy();
        });

        let click_controller = GestureClick::new();
        // 0 means all buttons
        click_controller.set_button(0);
        let terminal0 = terminal.clone();
        let ctrl_clicked = Rc::new(Cell::new(false));
        let ctrl_clicked_clone = ctrl_clicked.clone();
        click_controller.connect_pressed(move |controller, n_press, x, y| {
            if n_press == 1 {
                let button = controller.current_button();
                if button == GDK_BUTTON_PRIMARY as u32 {
                    let tmp = terminal0.check_match_at(x, y);
                    if let Some(hyper_link) = tmp.0 {
                        if ctrl_clicked_clone.get() {
                            println!("hyper_link: {}", hyper_link);
                            // Open the matched hyperlink with xdg-open on Linux
                            std::process::Command::new("xdg-open")
                                .arg(hyper_link)
                                .spawn()
                                .expect("Failed to open URL");
                        }
                    }
                }
            }
        });
        // Add the controller to the terminal
        terminal.add_controller(click_controller);

        let key_controller = EventControllerKey::new();
        let terminal_clone = terminal.clone();
        let font_step = 0.025;
        let opacity_step = 0.025;
        let window_clone = window.clone();
        let ctrl_clicked_clone = ctrl_clicked.clone();
        key_controller.connect_key_pressed(move |_controller, keyval, _keycode, state| {
            println!("connect_key_pressed state:{:?}, keyval: {}", state, keyval);
            if state == ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK {
                println!("keyval: {}", keyval);
                match keyval {
                    Key::C => {
                        println!("fuck: C {}", keyval);
                        terminal_clone.copy_clipboard_format(Format::Text);
                        return true.into();
                    }
                    Key::V => {
                        println!("fuck: V {}", keyval);
                        terminal_clone.paste_clipboard();
                        return true.into();
                    }
                    Key::plus => {
                        println!("fuck: plus {}", keyval);
                        terminal_font_scale.set((terminal_font_scale.get() + font_step).min(10.0));
                        terminal_clone.set_font_scale(terminal_font_scale.get());
                        return true.into();
                    }
                    Key::I => {
                        println!("fuck: I {}", keyval);
                        terminal_font_scale.set((terminal_font_scale.get() - font_step).max(0.1));
                        terminal_clone.set_font_scale(terminal_font_scale.get());
                        return true.into();
                    }
                    Key::O => {
                        println!("fuck: O {}", keyval);
                        terminal_font_scale.set((terminal_font_scale.get() + font_step).min(10.0));
                        terminal_clone.set_font_scale(terminal_font_scale.get());
                        return true.into();
                    }
                    Key::J => {
                        println!("fuck: J {}", keyval);
                        window_opacity.set((window_opacity.get() - opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity.get());
                        return true.into();
                    }
                    Key::K => {
                        println!("fuck: K {}", keyval);
                        window_opacity.set((window_opacity.get() + opacity_step).clamp(0.01, 1.0));
                        window_clone.set_opacity(window_opacity.get());
                        return true.into();
                    }
                    _ => {}
                }
            }
            if state == ModifierType::CONTROL_MASK {
                match keyval {
                    Key::minus => {
                        println!("fuck: minus {}", keyval);
                        terminal_font_scale.set((terminal_font_scale.get() - font_step).max(0.1));
                        terminal_clone.set_font_scale(terminal_font_scale.get());
                        return true.into();
                    }
                    _ => {}
                }
            }
            if state == ModifierType::NO_MODIFIER_MASK {
                if keyval == Key::Control_L || keyval == Key::Control_R {
                    ctrl_clicked_clone.set(true);
                    println!("ctrl clicked");
                }
            }
            false.into()
        });
        let ctrl_clicked_clone = ctrl_clicked.clone();
        key_controller.connect_key_released(move |_controller, _keyval, _keycode, state| {
            if state == ModifierType::CONTROL_MASK {
                println!("ctrl not clicked");
                ctrl_clicked_clone.set(false);
            }
        });
        terminal.add_controller(key_controller);

        let argv = &["/bin/bash", "-/bin/bash"];
        let envv: &[&str] = &[];
        let spawn_flags = SpawnFlags::SEARCH_PATH | SpawnFlags::FILE_AND_ARGV_ZERO;
        let cancellable: Option<&Cancellable> = None;
        let working_directory = Some("~/");
        terminal.spawn_async(
            PtyFlags::DEFAULT,
            working_directory,
            argv,
            envv,
            spawn_flags,
            || {},
            -1,
            cancellable,
            |res| println!("{:?}", res),
        );

        let app_clone = app.clone();
        window.connect_destroy(move |_| {
            app_clone.quit();
        });

        // Add the terminal to the application window
        window.set_child(Some(&terminal));

        // Show the window.
        window.show();
    });

    // Run the application
    app.run()
}
