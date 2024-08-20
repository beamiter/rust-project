use lazy_static::lazy_static;

use crate::dwm::{self, Rule};

pub const borderpx: u32 = 1;
pub const snap: u32 = 32;
pub const showbar: i32 = 1;
pub const topbar: i32 = 1;
lazy_static! {
    static ref fonts: Vec<&'static str> = vec!["monospace:size=10"];
    static ref rules: Vec<Rule> = vec![
        Rule::new("Gimp", "", "", 0, 1, -1),
        Rule::new("Firefox", "", "", 1 << 8, 0, -1)
    ];
}
pub const dmenufont: &str = "monospace:size=10";
pub const col_gray1: &str = "#222222";
pub const col_gray2: &str = "#444444";
pub const col_gray3: &str = "#bbbbbb";
pub const col_gray4: &str = "#eeeeee";
pub const col_cyan: &str = "#005577";

pub const colors: [&[&'static str; 3]; 2] = [
    &[col_gray3, col_gray1, col_gray2],
    &[col_gray4, col_cyan, col_cyan],
];

pub const tags: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];

pub const mfact: f32 = 0.55;
pub const nmaster: i32 = 1;
pub const resizehints: i32 = 1;
pub const lockfullscreen: i32 = 1;
