extern crate termion;

use std::{
    io::{stdin, stdout, Write},
    process::Command,
    vec,
};
use std::{thread, time};

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

// Currently limit the log number to 100.
fn get_git_log(branch: &String) -> Vec<String> {
    let output = Command::new("git")
        .args(["log", branch.as_str(), "-100"])
        .output()
        .expect("failed to execute process");
    // println!("status: {}", output.status);
    // assert!(output.status.success());
    // write!(stdout, "{:?}", String::from_utf8_lossy(&output.stdout)).unwrap();
    let log_output: Vec<char> = output.stdout.iter().map(|&t| t as char).collect();
    let mut log_iter = log_output.split(|&x| x == '\n');
    let mut logs: Vec<String> = vec![];
    loop {
        if let Some(val) = log_iter.next() {
            logs.push(val.iter().collect::<String>().to_string());
        } else {
            break;
        }
    }
    logs
}
fn show_git_log<W: Write>(
    screen: &mut W,
    branch: &String,
    zone_top: &mut usize,
    zone_bottom: &mut usize,
) {
    let (x, y) = screen.cursor_pos().unwrap();
    let logs = get_git_log(branch);
    let x_tmp = x + 30;
    let mut y_tmp = 2;
    // Clear previous log zone.
    for clear_y in *zone_top..=*zone_bottom {
        write!(
            screen,
            "{}{}",
            termion::cursor::Goto(x_tmp, clear_y as u16),
            termion::clear::UntilNewline,
        )
        .unwrap();
    }
    *zone_top = y_tmp as usize;
    for log in logs {
        if !log.is_empty() {
            write!(screen, "{}{}", termion::cursor::Goto(x_tmp, y_tmp), log).unwrap();
        }
        y_tmp += 1;
    }
    *zone_bottom = y_tmp as usize;
    write!(screen, "{}", termion::cursor::Goto(x, y)).unwrap();
}

fn get_git_branch() -> Vec<String> {
    let output = Command::new("git")
        .arg("branch")
        .output()
        .expect("failed to execute process");
    // println!("status: {}", output.status);
    // assert!(output.status.success());
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

fn checkout_git_branch<W: Write>(screen: &mut W, branch: &String, end_row: u16) -> bool {
    let output = Command::new("git")
        .args(["checkout", branch.as_str()])
        .output()
        .expect("failed to execute process");
    let (x, y) = screen.cursor_pos().unwrap();
    if !output.status.success() {
        write!(
            screen,
            "{}\u{1f602}{}{:?}{}{}",
            termion::cursor::Goto(x, end_row + 10),
            color::Fg(color::LightYellow),
            String::from_utf8_lossy(&output.stderr),
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y),
        )
        .unwrap();
    } else {
        write!(
            screen,
            "{}\u{1f973}{}Checkout to target branch {}{}{}, enter 'q' to quit{}{}",
            termion::cursor::Goto(x, end_row + 10),
            color::Fg(color::LightYellow),
            color::Fg(color::Green),
            branch,
            color::Fg(color::LightYellow),
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y),
        )
        .unwrap();
    }
    output.status.success()
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
        "{}{}{}{}",
        termion::cursor::Goto(1, bottom + 10),
        termion::clear::CurrentLine,
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
        "{}{}{}{}",
        termion::cursor::Goto(1, bottom + 10),
        termion::clear::CurrentLine,
        termion::cursor::Goto(1, *row),
        UNICODE_TABLE[*key_move_counter % UNICODE_TABLE.len()]
    )
    .unwrap();
    *key_move_counter = (*key_move_counter + 1) % usize::MAX;
}

fn main() {
    let mut branches = get_git_branch();

    let stdin = stdin();
    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    write!(
        screen,
        "{}{}{}Welcome to tui git{}{}{}\n",
        termion::cursor::Goto(20, 1),
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
    for branch in &mut branches {
        row += 1;
        if let Some('*') = branch.chars().next() {
            branch.remove(0);
            *branch = branch.trim().to_string();
            // This is the Current branch.
            write!(
                screen,
                "{}{}{}{}{}{}{}",
                termion::cursor::Goto(5, row),
                color::Bg(color::White),
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
    let mut zone_top = 2;
    let mut zone_bottom = 2;
    show_git_log(
        &mut screen,
        &branches.to_vec()[(row - start_row) as usize],
        &mut zone_top,
        &mut zone_bottom,
    );
    screen.flush().unwrap();
    let mut key_move_counter: usize = 0;
    for c in stdin.keys() {
        // write!(screen, "{}", termion::cursor::Hide).unwrap();
        match c.unwrap() {
            Key::Char('q') => break,
            Key::Char('\n') => {
                if checkout_git_branch(
                    &mut screen,
                    &branches.to_vec()[(row - start_row) as usize],
                    end_row,
                ) {
                    screen.flush().unwrap();
                    thread::sleep(time::Duration::from_secs_f32(0.5));
                    // break;
                }
            }
            Key::Up => {
                move_cursor_up(
                    &mut screen,
                    &mut row,
                    start_row,
                    end_row,
                    &mut key_move_counter,
                );
                // Show the log.
                show_git_log(
                    &mut screen,
                    &branches.to_vec()[(row - start_row) as usize],
                    &mut zone_top,
                    &mut zone_bottom,
                );
            }
            Key::Down => {
                move_cursor_down(
                    &mut screen,
                    &mut row,
                    start_row,
                    end_row,
                    &mut key_move_counter,
                );
                // Show the log.
                show_git_log(
                    &mut screen,
                    &branches.to_vec()[(row - start_row) as usize],
                    &mut zone_top,
                    &mut zone_bottom,
                );
            }
            _ => {}
        }
        screen.flush().unwrap();
    }
    // write!(screen, "{}", termion::cursor::Show).unwrap();
}
