use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use gdk::{Screen, RGBA};
use gtk::{prelude::*, AccelGroup, Window, WindowType};
use vte::{Terminal, TerminalExt};

trait TerminalTrait {
    fn term_activate_current_font(&self, window: &Window, font: &str, win_ready: bool) {}
    fn term_set_size(&self, window: &Window, width: i64, height: i64, win_ready: bool) {}
}

impl TerminalTrait for Terminal {
    fn term_activate_current_font(&self, window: &Window, font: &str, win_ready: bool) {
        let width = self.get_column_count();
        let height = self.get_row_count();

        let font_desc = pango::FontDescription::from_string(font);
        self.set_font(Some(font_desc).as_ref());
        self.set_font_scale(1.0);

        self.term_set_size(window, width, height, win_ready);
    }
    fn term_set_size(&self, window: &Window, width: i64, height: i64, win_ready: bool) {
        if width > 0 && height > 0 {
            self.set_size(width, height);
        }
        if win_ready {
            let (_, natural) = self.get_preferred_size();
            window.resize(natural.width, natural.height);
        }
    }
}

fn cb_spawn_async(_: &Terminal, _: glib::Pid, _: glib::Error) {}

fn main() {
    if gtk::init().is_err() {
        eprint!("failed to initialize gtk.");
        return;
    }

    let window = Window::new(WindowType::Toplevel);
    window.set_default_size(800, 600);
    window.set_widget_name("win_name");
    window.set_title("jterm3");

    let terminal = Terminal::new();
    terminal.set_widget_name("term_name");
    terminal.set_hexpand(true);
    terminal.set_vexpand(true);
    if let Some(screen) = Screen::get_default() {
        if let Some(visual) = screen.get_rgba_visual() {
            if screen.is_composited() {}
            window.set_visual(Some(visual).as_ref());
        }
    }
    window.add(&terminal);

    let counter = Arc::new(Mutex::new(0));
    let accel_group = AccelGroup::new();
    let counter_clone = counter.clone();
    accel_group.connect_accel_group(
        gdk::enums::key::E,
        gdk::ModifierType::CONTROL_MASK,
        gtk::AccelFlags::VISIBLE,
        move |_, _, _, _| {
            let mut num = counter_clone.lock().unwrap();
            *num += 1;
            println!("haha: {}", *num);
            Inhibit(false);
            false
        },
    );
    window.add_accel_group(&accel_group);

    terminal.term_activate_current_font(&window, "JetBrainsMono Nerd Font Regular 12", false);
    terminal.set_allow_bold(true);
    // terminal.set_bold_is_bright(true);
    terminal.set_opacity(0.9);
    terminal.set_cursor_blink_mode(vte::CursorBlinkMode::System);
    terminal.set_cursor_shape(vte::CursorShape::Ibeam);
    terminal.set_mouse_autohide(true);
    terminal.set_scrollback_lines(5000);
    // terminal.set_allow_hyperlink(true);
    let foreground = gdk::RGBA::from_str("#f8f7e9");
    terminal.set_color_foreground(&foreground.unwrap());
    let background = RGBA::from_str("#121616");
    terminal.set_color_background(&background.unwrap());
    let argv = &[
        std::path::Path::new("/bin/bash"),
        std::path::Path::new("-/bin/bash"),
    ];
    let envv: &[&std::path::Path] = &[];
    let spawn_flags = glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::FILE_AND_ARGV_ZERO;
    let cancellable: Option<&gio::Cancellable> = None;
    let mut binding = || {
        println!("Process terminated");
    };
    let child_setup: Option<&mut dyn (FnMut())> = Some(&mut binding);
    let _callback: Option<Box<dyn FnOnce(&Terminal, glib::Pid, &glib::Error) + 'static>> =
        Some(Box::new(
            |_terminal: &Terminal, pid: glib::Pid, error: &glib::Error| {
                println!("pid: {}", pid.0);
                println!("error: {:?}", error);
            },
        ));
    let working_directory = Some("~/");
    let _ = terminal.spawn_sync(
        vte::PtyFlags::DEFAULT,
        working_directory,
        argv,
        envv,
        spawn_flags,
        child_setup,
        cancellable,
    );
    // terminal.spawn_async(
    //     vte::PtyFlags::DEFAULT,
    //     working_directory,
    //     argv,
    //     envv,
    //     spawn_flags,
    //     child_setup,
    //     -1,
    //     cancellable,
    //     None,
    // );

    window.show_all();

    window.connect_delete_event(move |_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    gtk::main();
}
