extern crate termion;

use std::{
    io::{stdin, stdout, Write},
    process::Command,
};

use termion::input::TermRead;
use termion::{color, style};
use termion::{cursor::DetectCursorPos, event::Key};
use termion::{raw::IntoRawMode, screen::IntoAlternateScreen};

const UNICODE_TABLE: [&'static str; 12] = [
    "\u{1f407}",
    "\u{1f411}",
    "\u{1f412}",
    "\u{1f42f}",
    "\u{1f435}",
    "\u{1f436}",
    "\u{1f437}",
    "\u{1f981}",
    "\u{1f98a}",
    "\u{1f438}",
    "\u{1f33f}",
    "\u{1f341}",
];

fn get_git_log() {
    let output = Command::new("git")
        .arg("log")
        .output()
        .expect("failed to execute process");
    println!("status: {}", output.status);
    assert!(output.status.success());
    // println!("{}", String::from_utf8_lossy(&output.stdout));
}

fn get_git_branch() -> Vec<String> {
    let output = Command::new("git")
        .arg("branch")
        .output()
        .expect("failed to execute process");
    println!("status: {}", output.status);
    assert!(output.status.success());
    // println!("{}", String::from_utf8_lossy(&output.stdout));
    let mut branch_output: Vec<char> = output.stdout.iter().map(|&t| t as char).collect();
    branch_output.pop();
    // println!("branch_output {:?}", branch_output);
    let mut branch_iter = branch_output.split(|&x| x == '\n');
    let mut branches: Vec<String> = vec![];
    loop {
        if let Some(val) = branch_iter.next() {
            // println!("{}", val.iter().collect::<String>());
            branches.push(val.iter().collect::<String>().trim().to_string());
        } else {
            break;
        }
    }
    branches
}

fn checkout_git_branch<W: Write>(screen: &mut W, branch: &String) {
    let mut name = branch.to_string();
    if let Some('*') = name.chars().next() {
        name.remove(0);
    }
    name = name.trim().to_string();
    let output = Command::new("git")
        .args(["checkout", name.as_str()])
        .output()
        .expect("failed to execute process");
    let (x, y) = screen.cursor_pos().unwrap();
    if !output.status.success() {
        write!(
            screen,
            "{}\u{1f602}{}{:?}{}{}",
            termion::cursor::Goto(x, y + 10),
            color::Fg(color::LightYellow),
            String::from_utf8_lossy(&output.stderr),
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y),
        )
        .unwrap();
    } else {
        write!(
            screen,
            "{}\u{1f970}{}checkout to target branch {}{}{}",
            termion::cursor::Goto(x, y + 10),
            color::Fg(color::LightYellow),
            name,
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y),
        )
        .unwrap();
    }
}

// https://symbl.cc/en/
fn move_cursor_up<W: Write>(
    screen: &mut W,
    row: &mut u16,
    top: u16,
    bottom: u16,
    key_move_counter: &mut usize,
) {
    // Clear previous.
    write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
    if *row == top {
        *row = bottom;
    } else {
        *row = *row - 1;
    }
    write!(
        screen,
        "{}{}",
        termion::cursor::Goto(1, *row),
        UNICODE_TABLE[*key_move_counter % UNICODE_TABLE.len()]
    )
    .unwrap();
    *key_move_counter = (*key_move_counter + 1) % usize::MAX;
}
fn move_cursor_down<W: Write>(
    screen: &mut W,
    row: &mut u16,
    top: u16,
    bottom: u16,
    key_move_counter: &mut usize,
) {
    // Clear previous.
    write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
    if *row == bottom {
        *row = top;
    } else {
        *row = *row + 1;
    }
    write!(
        screen,
        "{}{}",
        termion::cursor::Goto(1, *row),
        UNICODE_TABLE[*key_move_counter % UNICODE_TABLE.len()]
    )
    .unwrap();
    *key_move_counter = (*key_move_counter + 1) % usize::MAX;
}

fn main() {
    get_git_log();
    let branches = get_git_branch();

    let stdin = stdin();
    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    write!(
        screen,
        "{}{}{}Welcome to tui git{}{}{}\n",
        termion::cursor::Goto(10, 1),
        color::Fg(color::Magenta),
        style::Bold,
        style::Italic,
        color::Fg(color::Reset),
        style::Reset,
    )
    .unwrap();
    // write!(screen, "{}{:?}", termion::cursor::Goto(1, 5), branches).unwrap();
    let start_row = 2;
    let mut row = start_row - 1;
    for branch in &branches {
        row += 1;
        if let Some('*') = branch.chars().next() {
            // This is the Current branch.
            write!(
                screen,
                "{}{}{}{}{}{}",
                termion::cursor::Goto(5, row),
                color::Fg(color::Green),
                style::Bold,
                branch,
                color::Fg(color::Reset),
                style::Reset,
            )
            .unwrap();
        } else {
            write!(screen, "{}{}", termion::cursor::Goto(5, row), branch,).unwrap();
        }
    }
    let end_row = start_row + branches.len() as u16 - 1;
    row = start_row;
    write!(screen, "{}{}", termion::cursor::Goto(1, row), "\u{1f63b}").unwrap();
    screen.flush().unwrap();
    let mut key_move_counter: usize = 0;
    for c in stdin.keys() {
        // write!(screen, "{}", termion::cursor::Hide).unwrap();
        match c.unwrap() {
            Key::Char('q') => break,
            Key::Char('\n') => {
                checkout_git_branch(&mut screen, &branches.to_vec()[(row - start_row) as usize])
            }
            // Key::Char(c) => println!("{}{}", termion::cursor::Goto(1, 3), c),
            // Key::Alt(c) => println!("{}^{}", termion::cursor::Goto(1, 3), c),
            // Key::Ctrl(c) => println!("{}*{}", termion::cursor::Goto(1, 3), c),
            // Key::Esc => println!("{}ESC", termion::cursor::Goto(1, 3),),
            // Key::Left => println!("{}←", termion::cursor::Goto(1, 3),),
            // Key::Right => println!("{}→", termion::cursor::Goto(1, 3),),
            Key::Up => move_cursor_up(
                &mut screen,
                &mut row,
                start_row,
                end_row,
                &mut key_move_counter,
            ),
            Key::Down => move_cursor_down(
                &mut screen,
                &mut row,
                start_row,
                end_row,
                &mut key_move_counter,
            ),
            // Key::Backspace => println!("{}×", termion::cursor::Goto(1, 3),),
            _ => {}
        }
        screen.flush().unwrap();
    }
    // write!(screen, "{}", termion::cursor::Show).unwrap();
}
