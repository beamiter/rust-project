// extern crate pango;
// extern crate pangocairo;
// extern crate x11;
// use std::fs::File;

// use cairo::{Context, Format, ImageSurface};
// use pango::{FontDescription, Layout};
// use pangocairo::functions::{create_context, show_layout, update_layout};

// fn main() {
//     // Create a Cairo surface to draw on, width: 200px, height: 100px
//     let surface = ImageSurface::create(Format::ARgb32, 200, 100).expect("Can't create a surface!");

//     // Create a Cairo context to draw with
//     let cr = Context::new(&surface);

//     // Create a Pango context using the default font map
//     let pango_context = create_context(cr.as_ref().unwrap());

//     // Create a Pango layout for the text
//     let layout = Layout::new(&pango_context);

//     // Set the text properties
//     let font_description = FontDescription::from_string("Sans Bold 12");
//     layout.set_font_description(Some(&font_description));
//     layout.set_text("🍇🍵🎦🎮🎵🏖🐣🐶🦄");

//     // Render the text
//     update_layout(cr.as_ref().unwrap(), &layout);
//     show_layout(cr.as_ref().unwrap(), &layout);

//     // Write the result to a PNG file
//     // Open a file in write-only mode to write the PNG data
//     let mut file = File::create("output.png").unwrap();
//     let _ = surface.write_to_png(&mut file);

//     // In a real X11 application, instead of writing to a PNG,
//     // you would create an X11 surface and pass it to the Cairo context.
// }

extern crate cairo;
extern crate pango;
extern crate pangocairo;

use cairo::{Context, Surface};
use cairo_sys::cairo_xlib_surface_create;
use pango::{FontDescription, Layout};
use pangocairo::functions::{create_context, show_layout, update_layout};

use std::ptr;
use x11::xlib;

fn main() {
    unsafe {
        // Open a connection to the X server
        let display = xlib::XOpenDisplay(ptr::null());

        // Create a simple window
        let screen = xlib::XDefaultScreen(display);
        let root = xlib::XRootWindow(display, screen);
        let win = xlib::XCreateSimpleWindow(
            display,
            root,
            0,
            0,
            200,
            100,
            0,
            xlib::XBlackPixel(display, screen),
            xlib::XWhitePixel(display, screen),
        );

        // Map the window (make it visible)
        xlib::XMapWindow(display, win);

        // Flush the output buffer and wait until all requests have been processed by the server
        xlib::XFlush(display);

        // Create a Cairo surface that represents the window
        let xlib_surface = cairo_xlib_surface_create(
            display,
            win,
            xlib::XDefaultVisual(display, screen) as *mut _,
            200,
            100,
        );
        let surface = Surface::from_raw_none(xlib_surface);

        // Create a Cairo context to draw with
        let cr = Context::new(&surface).unwrap();

        // Create a Pango context using the default font map
        let pango_context = create_context(&cr);

        // Create a Pango layout for the text
        let layout = Layout::new(&pango_context);

        // Set the text properties
        let font_description = FontDescription::from_string("Sans Bold 12");
        layout.set_font_description(Some(&font_description));
        layout.set_text("Hello, PangoCairo!");

        // Render the text
        update_layout(&cr, &layout);
        show_layout(&cr, &layout);

        // Flush drawing actions
        surface.flush();
        let _ = cr.show_page();

        // Wait for a key press before exiting
        xlib::XSelectInput(display, win, xlib::KeyPressMask);
        let mut event: xlib::XEvent = std::mem::zeroed();
        xlib::XNextEvent(display, &mut event);

        // Clean up resources
        xlib::XDestroyWindow(display, win);
        xlib::XCloseDisplay(display);
    }
}
