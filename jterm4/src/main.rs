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
use vte4::{CursorBlinkMode, CursorShape, PtyFlags, Terminal};
use vte4::{TerminalExt, TerminalExtManual};

fn main() -> glib::ExitCode {
    // Create a new GTK application
    let app = Application::builder().application_id("app.jterm4").build();

    // Connect to the "activate" signal of the application
    app.connect_activate(|app| {
        // Create a new application window
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(320)
            .default_height(200)
            .title("jterm4")
            .name("win_name")
            .opacity(0.8)
            .build();

        // Create a new VTE terminal widget
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
            .font_scale(1.0)
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
        // Set teh terminal's font
        terminal.set_font(Some(&font_desc));

        // (TODO) has bug
        let regex_pattern = vte4::Regex::for_match(r"[a-z]+://[[:graph:]]+", 0);
        // let regex_pattern = vte::Regex::new(r"https?://[^\s]+", 0);
        terminal.match_add_regex(&regex_pattern.unwrap(), 0);

        terminal.connect_bell(move |_| {
            println!("Bell signal received");
        });

        let window_clone = window.clone();
        terminal.connect_child_exited(move |_, _| {
            window_clone.destroy();
        });

        let click_controller = GestureClick::new();
        // 0 means all buttons
        click_controller.set_button(0);
        click_controller.connect_pressed(move |controller, n_press, _x, _y| {
            // println!("n_press: {}", n_press);
            if n_press == 1 {
                let button = controller.current_button();
                // println!("button: {}", button);
                if button == GDK_BUTTON_PRIMARY as u32 {}
            }
        });
        // Add the controller to the terminal
        // terminal.add_controller(click_controller);

        let key_controller = EventControllerKey::new();
        key_controller.connect_key_pressed(move |_controller, keyval, _keycode, state| {
            println!("connect_key_pressed state:{:?}, keyval: {}", state, keyval);
            if state == ModifierType::CONTROL_MASK {
                // println!("wtf0");
            }
            if keyval == Key::c || keyval == Key::v {
                // println!("wtf1");
            }
            false.into()
        });
        key_controller.connect_key_released(move |_controller, keyval, _keycode, state| {
            // println!("connect_key_released state:{:?}, keyval: {}", state, keyval);
            if state == ModifierType::CONTROL_MASK {
                // println!("wtf0");
            }
            if keyval == Key::c || keyval == Key::v {
                // println!("wtf1");
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
            || println!("haha"),
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
