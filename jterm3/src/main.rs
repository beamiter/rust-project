use gtk::{prelude::*, Window, WindowType};
use vte::Terminal;

fn main() {
    if gtk::init().is_err() {
        eprint!("failed to initialize gtk.");
        return;
    }

    let window = Window::new(WindowType::Toplevel);
    window.set_default_size(800, 600);

    let terminal = Terminal::new();

    window.add(&terminal);

    window.show_all();

    window.connect_delete_event(move |_, _| {
        gtk::main_quit();
        Inhibit(false)
    });

    gtk::main();
}
