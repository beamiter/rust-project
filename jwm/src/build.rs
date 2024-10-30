// build.rs
// https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib
// https://doc.rust-lang.org/rustc/command-line-arguments.html#option-l-link-lib

fn main() {
    // Link against the `X11` library on Unix-like systems
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=fontconfig");
    println!("cargo:rustc-link-lib=Xinerama");
    println!("cargo:rustc-link-lib=Xft");

    // For dynamic linking with shared libraries, you can add "=dylib":
    // println!("cargo:rustc-link-lib=dylib=X11");

    // For static linking, you would add "=static":
    // println!("cargo:rustc-link-lib=static=X11");

    // To specify additional system-specific or custom libraries, just add more lines:
    // println!("cargo:rustc-link-lib=ssl");
    // println!("cargo:rustc-link-lib=crypto");
}
