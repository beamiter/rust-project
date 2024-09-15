mod config;
mod drw;
mod dwm;
mod xproto;

fn main() {
    let str = "\0";
    println!("Hello, world, {}", str.chars().nth(0).unwrap());
}
