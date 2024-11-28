// build.rs
// https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib
// https://doc.rust-lang.org/rustc/command-line-arguments.html#option-l-link-lib

use std::env;
use std::path::PathBuf;

pub const BUILD_IN_NIX_SHELL: bool = true;

#[allow(unreachable_code)]
fn main() {
    // Link against the `X11` library on Unix-like systems
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=fontconfig");
    println!("cargo:rustc-link-lib=Xinerama");
    println!("cargo:rustc-link-lib=Xft");
    println!("cargo:rustc-link-lib=Xrender");
    // Use pkg_config to find and link the Pango library.
    match pkg_config::find_library("pango") {
        Ok(_) => {}
        Err(e) => panic!("Couldn't find pango library: {:?}", e),
    }
    match pkg_config::find_library("pangoxft") {
        Ok(_) => {}
        Err(e) => panic!("Couldn't find pango library: {:?}", e),
    }
    return;

    // For dynamic linking with shared libraries, you can add "=dylib":
    // println!("cargo:rustc-link-lib=dylib=X11");

    // For static linking, you would add "=static":
    // println!("cargo:rustc-link-lib=static=X11");

    // To specify additional system-specific or custom libraries, just add more lines:
    // println!("cargo:rustc-link-lib=ssl");
    // println!("cargo:rustc-link-lib=crypto");

    println!("cargo:rerun-if-changed=wrapper.h");

    // If build in nix environment.
    if BUILD_IN_NIX_SHELL {
        let clang_path = "/nix/store/p3bv60x7rzlnfz7ms7i1rm5ps0481idg-clang-18.1.8-lib/lib/";
        let path_buf = PathBuf::from(clang_path);
        if path_buf.as_path().exists() && path_buf.as_path().is_dir() {
            env::set_var("LIBCLANG_PATH", path_buf);
        }
    }

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        // Add necessary clang args, such as include paths or defines
        .clang_arg("-I/usr/include/pango-1.0") // This path might differ
        .clang_arg("-I/usr/include/freetype2") // These are example paths
        .clang_arg("-I/usr/include/xft") // Adjust them for your system
        .clang_arg("-I/usr/include/glib-2.0")
        .clang_arg("-I/usr/lib64/glib-2.0/include")
        .clang_arg("-I/usr/include/harfbuzz")
        .clang_arg("-I/usr/lib/x86_64-linux-gnu/glib-2.0/include")
        .blocklist_item("_XDisplay")
        .raw_line("pub use x11::xlib::_XDisplay;")
        .blocklist_type("_PangoFontMap") // Replace with the specific type name as needed
        .raw_line("type _PangoFontMap = pango::ffi::PangoFontMap;")
        .blocklist_type("_XftDraw") // Replace with the specific type name as needed
        .raw_line("type _XftDraw = x11::xft::XftDraw;")
        .blocklist_type("_XftColor") // Replace with the specific type name as needed
        .raw_line("type _XftColor = x11::xft::XftColor;")
        .blocklist_type("_PangoLayout") // Replace with the specific type name as needed
        .raw_line("type _PangoLayout = pango::ffi::PangoLayout;")
        // Generate bindings for functions and types used by PangoXft
        .allowlist_function("pango_xft_.*")
        .allowlist_type("PangoXft.*")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
