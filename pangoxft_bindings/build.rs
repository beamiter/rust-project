// build.rs

use std::env;
use std::path::PathBuf;

fn main() {
    // The PangoXft headers and library are needed to generate the bindings.
    // Ensure that they are installed on your system.
    println!("cargo:rerun-if-changed=wrapper.h");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        // .raw_line("#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]")
        // Add necessary clang args, such as include paths or defines
        .clang_arg("-I/usr/include/pango-1.0") // This path might differ
        .clang_arg("-I/usr/include/freetype2") // These are example paths
        .clang_arg("-I/usr/include/xft") // Adjust them for your system
        .clang_arg("-I/usr/include/glib-2.0")
        .clang_arg("-I/usr/include/harfbuzz")
        .clang_arg("-I/usr/lib/x86_64-linux-gnu/glib-2.0/include")
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
