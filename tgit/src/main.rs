extern crate termion;

use std::{
    collections::HashMap,
    io::{stdin, stdout, Write},
    process::Command,
    vec,
};
use substring::Substring;

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

struct TuiGit {
    // branch render area;
    branch_row_top: u16,
    branch_row_bottom: u16,
    branch_col_left: u16,
    branch_col_right: u16,

    // log render area;
    log_row_top: u16,
    log_row_bottom: u16,
    log_col_left: u16,
    // log_col_right: u16,
    //
    // // check status area;
    // check_info_row: u16,
    // check_info_col: u16,

    // data storage;
    branch_vec: Vec<String>,
    log_map: HashMap<String, Vec<String>>,

    // Main branch;
    main_branch: String,
    main_row: u16,
}
impl TuiGit {
    pub fn new() -> TuiGit {
        TuiGit {
            branch_row_top: 2,
            branch_row_bottom: 0,
            branch_col_left: 5,
            branch_col_right: 0,
            log_row_top: 1,
            log_row_bottom: 0,
            log_col_left: 0,
            // log_col_right: 0,
            // check_info_row: 0,
            // check_info_col: 0,
            branch_vec: vec![],
            log_map: HashMap::new(),
            main_branch: String::new(),
            main_row: 0,
        }
    }
    fn set_main_row(&mut self, row: u16) {
        self.main_row = row;
    }
    fn update_git_branch(&mut self) {
        let output = Command::new("git")
            .arg("branch")
            .output()
            .expect("failed to execute process");
        let branch_output: Vec<char> = output
            .stdout
            .iter()
            .map(|&t| t as char)
            .filter(|&t| t != ' ')
            .collect();
        // println!("branch_output {:?}", branch_output);
        let mut branch_iter = branch_output.split(|&x| x == '\n');
        self.branch_vec.clear();
        loop {
            if let Some(val) = branch_iter.next() {
                if val.is_empty() {
                    continue;
                }
                // println!("{}", val.iter().collect::<String>());
                if let Some('*') = val.iter().next() {
                    self.main_branch = val.iter().collect::<String>();
                    // Remove the '*' symbol.
                    self.main_branch.remove(0);
                    let head_str: &str = "HEADdetachedat";
                    if let Some(pos) = self.main_branch.find(head_str) {
                        self.main_branch = self
                            .main_branch
                            .substring(pos + head_str.len(), self.main_branch.len() - 1)
                            .to_string();
                    }
                    println!("{}", self.main_branch);
                    self.branch_vec.push(self.main_branch.to_string());
                } else {
                    self.branch_vec
                        .push(val.iter().collect::<String>().to_string());
                }
            } else {
                break;
            }
        }
        let branch_size = self
            .branch_vec
            .iter()
            .map(|x| x.len())
            .collect::<Vec<usize>>();
        self.branch_row_bottom = self.branch_vec.len() as u16 + self.branch_row_top - 1;
        self.branch_col_right =
            self.branch_col_left + *branch_size.iter().max().unwrap() as u16 + 5;
        self.log_col_left = self.branch_col_right + 5;
        println!(
            "{}--{}--{}",
            branch_size.iter().max().unwrap(),
            self.branch_row_top,
            self.branch_row_bottom
        );
    }

    // Currently limit the log number to 100.
    fn update_git_log(&mut self, branch: &String) {
        if self.log_map.get(branch).is_some() {
            return;
        }
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
        self.log_map.insert(branch.to_string(), logs);
    }
}

trait RenderGit {
    fn show_title<W: Write>(&mut self, screen: &mut W);
    fn show_branch<W: Write>(&mut self, screen: &mut W);
    fn show_git_log<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn checkout_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn cursor_to_main<W: Write>(&self, screen: &mut W);

    fn move_cursor_up<W: Write>(&self, screen: &mut W, row: &mut u16, key_move_counter: &mut usize);
    fn move_cursor_down<W: Write>(
        &self,
        screen: &mut W,
        row: &mut u16,
        key_move_counter: &mut usize,
    );
}

impl RenderGit for TuiGit {
    fn show_git_log<W: Write>(&mut self, screen: &mut W, branch: &String) {
        let (x, y) = screen.cursor_pos().unwrap();
        self.update_git_log(branch);
        let (col, row) = termion::terminal_size().unwrap();
        let x_tmp = self.log_col_left;
        if col <= x_tmp {
            // No show due to no enough col.
            return;
        }
        let mut y_tmp = 2;
        // Clear previous log zone.
        for clear_y in self.log_row_top..=self.log_row_bottom {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(x_tmp, clear_y as u16),
                termion::clear::UntilNewline,
            )
            .unwrap();
        }
        self.log_row_top = y_tmp;
        for log in self.log_map.get(branch).unwrap() {
            if !log.is_empty() {
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(x_tmp, y_tmp as u16),
                    log.substring(0, (col - x_tmp) as usize)
                )
                .unwrap();
            }
            // Spare 2 for check info.
            if y_tmp >= row - 2 {
                break;
            }
            y_tmp += 1;
        }
        self.log_row_bottom = y_tmp;
        write!(screen, "{}", termion::cursor::Goto(x, y)).unwrap();
    }
    fn checkout_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool {
        let (x, y) = screen.cursor_pos().unwrap();
        let (_, row) = termion::terminal_size().unwrap();
        if branch == &self.main_branch {
            write!(
                screen,
                "{}{}\u{1f973}{}Already in target branch {}{}{}, enter 'q' to quit{}{}",
                termion::cursor::Goto(1, row),
                termion::clear::All,
                color::Fg(color::LightYellow),
                color::Fg(color::Green),
                branch,
                color::Fg(color::LightYellow),
                color::Fg(color::Reset),
                termion::cursor::Goto(x, y),
            )
            .unwrap();
            return true;
        }
        let output = Command::new("git")
            .args(["checkout", branch.as_str()])
            .output()
            .expect("failed to execute process");
        if !output.status.success() {
            write!(
                screen,
                "{}{}\u{1f602}{}{:?}{}{}",
                termion::cursor::Goto(1, row),
                termion::clear::All,
                color::Fg(color::LightYellow),
                String::from_utf8_lossy(&output.stderr),
                color::Fg(color::Reset),
                termion::cursor::Goto(x, y),
            )
            .unwrap();
        } else {
            self.main_branch = branch.to_string();
            self.main_row = y;
            write!(
                screen,
                "{}{}\u{1f973}{}Checkout to target branch {}{}{}, enter 'q' to quit{}{}",
                termion::cursor::Goto(1, row),
                termion::clear::BeforeCursor,
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

    fn cursor_to_main<W: Write>(&self, screen: &mut W) {
        write!(
            screen,
            "{}\u{1f33f}",
            termion::cursor::Goto(1, self.main_row)
        )
        .unwrap();
    }
    // https://symbl.cc/en/
    fn move_cursor_up<W: Write>(
        &self,
        screen: &mut W,
        row: &mut u16,
        key_move_counter: &mut usize,
    ) {
        // Clear previous.
        write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
        if *row == self.branch_row_top {
            *row = self.branch_row_bottom;
        } else {
            *row = *row - 1;
        }
        write!(
            screen,
            "{}{}{}{}",
            termion::cursor::Goto(1, self.branch_row_bottom + 10),
            termion::clear::CurrentLine,
            termion::cursor::Goto(1, *row),
            UNICODE_TABLE[*key_move_counter % UNICODE_TABLE.len()]
        )
        .unwrap();
        *key_move_counter = (*key_move_counter + 1) % usize::MAX;
    }
    fn move_cursor_down<W: Write>(
        &self,
        screen: &mut W,
        row: &mut u16,
        key_move_counter: &mut usize,
    ) {
        // Clear previous.
        write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
        if *row == self.branch_row_bottom {
            *row = self.branch_row_top;
        } else {
            *row = *row + 1;
        }
        write!(
            screen,
            "{}{}{}{}",
            termion::cursor::Goto(1, self.branch_row_bottom + 10),
            termion::clear::CurrentLine,
            termion::cursor::Goto(1, *row),
            UNICODE_TABLE[*key_move_counter % UNICODE_TABLE.len()]
        )
        .unwrap();
        *key_move_counter = (*key_move_counter + 1) % usize::MAX;
    }

    fn show_title<W: Write>(&mut self, screen: &mut W) {
        write!(
            screen,
            "{}{}{}{}Welcome to tui git{}{}{}\n",
            termion::cursor::Goto(19, 1),
            termion::clear::CurrentLine,
            color::Fg(color::Magenta),
            style::Bold,
            style::Italic,
            color::Fg(color::Reset),
            style::Reset,
        )
        .unwrap();
    }

    fn show_branch<W: Write>(&mut self, screen: &mut W) {
        let mut row = 1;
        for branch in self.branch_vec.to_vec() {
            row += 1;
            if *branch == self.main_branch {
                self.set_main_row(row);
                write!(
                    screen,
                    "{}{}{}{}{}{}{}{} \u{1f63b}",
                    termion::cursor::Goto(4, row),
                    termion::clear::CurrentLine,
                    color::Bg(color::White),
                    color::Fg(color::Green),
                    style::Bold,
                    branch,
                    color::Fg(color::Reset),
                    style::Reset,
                )
                .unwrap();
            } else {
                write!(
                    screen,
                    "{}{}{}",
                    termion::cursor::Goto(4, row),
                    termion::clear::CurrentLine,
                    branch
                )
                .unwrap();
            }
        }
    }
}

fn main() {
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();

    let stdin = stdin();
    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    write!(screen, "{}", termion::cursor::Hide).unwrap();

    // Init show, need to refresh every frame.
    tui_git.show_title(&mut screen);
    tui_git.show_branch(&mut screen);
    tui_git.cursor_to_main(&mut screen);
    tui_git.show_git_log(&mut screen, &tui_git.main_branch.to_string());
    screen.flush().unwrap();

    let mut key_move_counter: usize = 0;
    let mut main_row = tui_git.main_row;
    for c in stdin.keys() {
        match c.unwrap() {
            Key::Char('q') => break,
            Key::Char('\n') => {
                let branch =
                    &mut tui_git.branch_vec.to_vec()[(main_row - tui_git.branch_row_top) as usize];
                if tui_git.checkout_git_branch(&mut screen, branch) {
                    // break;
                } else {
                    *branch = tui_git.main_branch.to_string();
                }
                // Refresh title and branch.
                tui_git.show_title(&mut screen);
                tui_git.show_branch(&mut screen);
                tui_git.cursor_to_main(&mut screen);
                main_row = tui_git.main_row;
                tui_git.show_git_log(&mut screen, branch);
                screen.flush().unwrap();
            }
            Key::Up => {
                tui_git.move_cursor_up(&mut screen, &mut main_row, &mut key_move_counter);
                // Show the log.
                tui_git.show_git_log(
                    &mut screen,
                    &tui_git.branch_vec.to_vec()[(main_row - tui_git.branch_row_top) as usize],
                );
            }
            Key::Down => {
                tui_git.move_cursor_down(&mut screen, &mut main_row, &mut key_move_counter);
                // Show the log.
                tui_git.show_git_log(
                    &mut screen,
                    &tui_git.branch_vec.to_vec()[(main_row - tui_git.branch_row_top) as usize],
                );
            }
            _ => {}
        }
        screen.flush().unwrap();
    }
}
