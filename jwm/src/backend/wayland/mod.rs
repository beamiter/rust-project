pub mod backend;
pub mod color;
pub mod cursor;
pub mod event_source;
pub mod ewmh_facade;
pub mod input_ops;
pub mod key_ops;
pub mod output_ops;
pub mod property_ops;
pub mod window_ops;

pub mod grabs;
pub mod handlers;
pub mod input;
pub mod state;
pub mod winit;

use smithay::reexports::{
    calloop::EventLoop,
    wayland_server::{Display, DisplayHandle},
};

pub use state::Smallvil;

pub struct CalloopData {
    state: Smallvil,
    display_handle: DisplayHandle,
}

pub fn test_main() -> Result<(), Box<dyn std::error::Error>> {
    let mut event_loop: EventLoop<CalloopData> = EventLoop::try_new()?;
    let display: Display<Smallvil> = Display::new()?;
    let display_handle = display.handle();
    let state = Smallvil::new(&mut event_loop, display);
    let mut data = CalloopData {
        state,
        display_handle,
    };
    winit::init_winit(&mut event_loop, &mut data)?;
    let mut args = std::env::args().skip(1);
    let flag = args.next();
    let arg = args.next();
    match (flag.as_deref(), arg) {
        (Some("-c") | Some("--command"), Some(command)) => {
            std::process::Command::new(command).spawn().ok();
        }
        _ => {
            std::process::Command::new("terminator").spawn().ok();
        }
    }
    event_loop.run(None, &mut data, move |_| {
        // Smallvil is running
    })?;
    Ok(())
}
