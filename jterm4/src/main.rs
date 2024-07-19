use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4 as gtk;
use gtk4::gio::Cancellable;
use gtk4::glib::SpawnFlags;
use vte4::TerminalExtManual;
use vte4::{PtyFlags, Terminal};

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id("app.jterm4").build();

    app.connect_activate(|app| {
        // We create the main window.
        let window = ApplicationWindow::builder()
            .application(app)
            .default_width(320)
            .default_height(200)
            .title("jterm4")
            .name("win_name")
            .opacity(0.8)
            .build();

        let terminal = Terminal::builder()
            .hexpand(true)
            .vexpand(true)
            .name("term_name")
            .allow_hyperlink(true)
            .bold_is_bright(true)
            .scrollback_lines(5000)
            .opacity(0.5)
            .pointer_autohide(true)
            .build();
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

        window.set_child(Some(&terminal));

        // Show the window.
        window.present();
    });

    app.run()
}
