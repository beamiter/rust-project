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
    log_scroll_offset: u16,
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
    main_branch_row: u16,
    current_branch: String,
    current_branch_row: u16,

    // 0 for title, 1 for branch, 2 for log and e.t.c.
    layout_position: u16,
    key_move_counter: usize,
}
impl TuiGit {
    pub fn new() -> TuiGit {
        TuiGit {
            branch_row_top: 2,
            branch_row_bottom: 0,
            branch_col_left: 5,
            branch_col_right: 0,
            log_row_top: 2,
            log_row_bottom: 0,
            log_col_left: 0,
            log_scroll_offset: 0,
            // log_col_right: 0,
            // check_info_row: 0,
            // check_info_col: 0,
            branch_vec: vec![],
            log_map: HashMap::new(),
            main_branch: String::new(),
            main_branch_row: 0,
            current_branch: String::new(),
            current_branch_row: 0,
            layout_position: 0,
            key_move_counter: 0,
        }
    }
    fn set_main_branch_row(&mut self, row: u16) {
        self.main_branch_row = row;
    }
    fn update_git_branch(&mut self) {
        let output = Command::new("git")
            .arg("branch")
            .output()
            .expect("failed to execute process");
        let branch_output = String::from_utf8_lossy(&output.stdout);
        // println!("branch_output {:?}", branch_output);
        let mut branch_iter = branch_output.split('\n');
        self.branch_vec.clear();
        loop {
            if let Some(val) = branch_iter.next() {
                if val.is_empty() {
                    continue;
                }
                if let Some('*') = val.chars().next() {
                    // Remove the '*' symbol and trim white space.
                    self.main_branch = val[1..val.len()].to_string().trim().to_string();
                    let head_str: &str = "HEAD detached at ";
                    if let Some(pos) = self.main_branch.find(head_str) {
                        self.main_branch = self
                            .main_branch
                            .substring(pos + head_str.len(), self.main_branch.len() - 1)
                            .to_string();
                    }
                    println!("Main branch: {}", self.main_branch);
                    self.update_git_log(&self.main_branch.to_string());
                    self.branch_vec.push(self.main_branch.to_string());
                } else {
                    self.branch_vec.push(val.to_string().trim().to_string());
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
        // write!(stdout(), "{:?}", String::from_utf8_lossy(&output.stdout)).unwrap();
        let log_output = String::from_utf8_lossy(&output.stdout);
        let mut log_iter = log_output.split('\n');
        let mut logs: Vec<String> = vec![];
        loop {
            if let Some(val) = log_iter.next() {
                logs.push(val.to_string());
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

    fn refresh_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn enter_pressed<W: Write>(&mut self, screen: &mut W, row: &mut u16);

    fn move_cursor_left<W: Write>(&mut self, screen: &mut W, row: &mut u16);
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W, row: &mut u16);

    fn move_cursor_up<W: Write>(&mut self, screen: &mut W, row: &mut u16);
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W, row: &mut u16);
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
        // Clear previous log zone.
        self.log_row_bottom = (self.log_map.get(branch).unwrap().len() as u16).min(row - 2);
        for clear_y in self.log_row_top..=self.log_row_bottom {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(x_tmp, clear_y as u16),
                termion::clear::UntilNewline,
            )
            .unwrap();
        }
        let mut y_tmp = self.log_row_top;
        let current_branch_log_len = self.log_map.get(branch).unwrap().len() as u16;
        assert!(self.log_scroll_offset >= 0 && self.log_scroll_offset < current_branch_log_len);
        for log in &self.log_map.get(branch).unwrap().to_vec()
            [self.log_scroll_offset as usize..current_branch_log_len as usize]
        {
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
        self.current_branch = branch.to_string();
        self.current_branch_row = y;
        write!(screen, "{}", termion::cursor::Goto(x, y)).unwrap();
    }
    fn checkout_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool {
        let (x, y) = screen.cursor_pos().unwrap();
        let (_, row) = termion::terminal_size().unwrap();
        if branch == &self.main_branch {
            write!(
                screen,
                "{}{}☑{} Already in target branch {}{}{}, enter 'q' to quit{}{}",
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
                "{}{}❌{} {:?}{}{}",
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
            self.main_branch_row = y;
            write!(
                screen,
                "{}{}✅{} Checkout to target branch {}{}{}, enter 'q' to quit{}{}",
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
            "{}🌟",
            termion::cursor::Goto(1, self.main_branch_row)
        )
        .unwrap();
    }
    // https://symbl.cc/en/
    fn move_cursor_up<W: Write>(&mut self, screen: &mut W, row: &mut u16) {
        match self.layout_position {
            1 => {
                // Clear previous.
                write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
                if *row == self.branch_row_top {
                    *row = self.branch_row_bottom;
                } else {
                    *row = *row - 1;
                }
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(1, *row),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
                )
                .unwrap();
                // Show the log.
                self.show_git_log(
                    screen,
                    &self.branch_vec.to_vec()[(*row - self.branch_row_top) as usize],
                );
            }
            2 => {
                // Clear previous.
                write!(
                    screen,
                    "{} ",
                    termion::cursor::Goto(self.log_col_left - 2, *row)
                )
                .unwrap();
                if *row == self.log_row_top {
                    // For a close loop browse.
                    // *row = self.log_row_bottom;
                    // Hit the top.
                    if self.log_scroll_offset > 0 {
                        self.log_scroll_offset -= 1;
                        self.show_git_log(screen, &self.current_branch.to_string());
                    }
                } else {
                    *row = *row - 1;
                }
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(self.log_col_left - 2, *row),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
                )
                .unwrap();
            }
            _ => {}
        }

        self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
    }
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W, row: &mut u16) {
        match self.layout_position {
            1 => {
                // Clear previous.
                write!(screen, "{} ", termion::cursor::Goto(1, *row)).unwrap();
                if *row == self.branch_row_bottom {
                    *row = self.branch_row_top;
                } else {
                    *row = *row + 1;
                }
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(1, *row),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
                )
                .unwrap();
                // Show the log.
                self.show_git_log(
                    screen,
                    &self.branch_vec.to_vec()[(*row - self.branch_row_top) as usize],
                );
            }
            2 => {
                // Clear previous.
                write!(
                    screen,
                    "{} ",
                    termion::cursor::Goto(self.log_col_left - 2, *row)
                )
                .unwrap();
                if *row == self.log_row_bottom {
                    // For a close loop browse.
                    // *row = self.log_row_top;
                    // Hit the bottom.
                    let log_show_range = self.log_row_bottom - self.log_row_top;
                    let current_log_len = self.log_map.get(&self.current_branch).unwrap().len();
                    if usize::from(self.log_scroll_offset + log_show_range) < current_log_len {
                        self.log_scroll_offset += 1;
                        self.show_git_log(screen, &self.current_branch.to_string());
                    }
                } else {
                    *row = *row + 1;
                }
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(self.log_col_left - 2, *row),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
                )
                .unwrap();
            }
            _ => {}
        }

        self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
    }
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W, row: &mut u16) {
        self.layout_position = 2;
        let (_, y) = screen.cursor_pos().unwrap();
        write!(screen, "{}  ", termion::cursor::Goto(1, y)).unwrap();
        write!(
            screen,
            "{}✍ ",
            termion::cursor::Goto(self.log_col_left - 2, self.log_row_top)
        )
        .unwrap();
        *row = self.log_row_top;
    }
    fn move_cursor_left<W: Write>(&mut self, screen: &mut W, row: &mut u16) {
        self.layout_position = 1;
        *row = self.current_branch_row;
        let (x, y) = screen.cursor_pos().unwrap();
        write!(screen, "{}  ", termion::cursor::Goto(x - 2, y)).unwrap();
        write!(
            screen,
            "{}  ",
            termion::cursor::Goto(self.log_col_left - 2, self.current_branch_row)
        )
        .unwrap();
        write!(
            screen,
            "{}❆ ",
            termion::cursor::Goto(1, self.current_branch_row)
        )
        .unwrap();
    }

    fn show_title<W: Write>(&mut self, screen: &mut W) {
        self.layout_position = 0;
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
        self.layout_position = 1;
        let mut row = 1;
        for branch in self.branch_vec.to_vec() {
            row += 1;
            if *branch == self.main_branch {
                self.set_main_branch_row(row);
                write!(
                    screen,
                    "{}{}{}{}{}{}{}{} 🐝",
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

    fn refresh_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String) {
        self.show_title(screen);
        self.show_branch(screen);
        self.cursor_to_main(screen);
        self.show_git_log(screen, branch);
        screen.flush().unwrap();
    }

    fn enter_pressed<W: Write>(&mut self, screen: &mut W, row: &mut u16) {
        match self.layout_position {
            1 => {
                let branch = &mut self.branch_vec.to_vec()[(*row - self.branch_row_top) as usize];
                if self.checkout_git_branch(screen, branch) {
                } else {
                    *branch = self.main_branch.to_string();
                }
                // Refresh title and branch.
                self.refresh_with_branch(screen, branch);
                *row = self.main_branch_row;
            }
            2 => {}
            _ => {}
        }
    }
}

fn main() {
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();
    // return;

    let stdin = stdin();
    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    write!(screen, "{}", termion::cursor::Hide).unwrap();

    tui_git.refresh_with_branch(&mut screen, &tui_git.main_branch.to_string());

    // Start with the main branch row.
    let mut row = tui_git.main_branch_row;
    for c in stdin.keys() {
        match c.unwrap() {
            Key::Char('q') => break,
            Key::Char('\n') => {
                tui_git.enter_pressed(&mut screen, &mut row);
            }
            Key::Left => {
                tui_git.move_cursor_left(&mut screen, &mut row);
            }
            Key::Right => {
                tui_git.move_cursor_right(&mut screen, &mut row);
            }
            Key::Up => {
                tui_git.move_cursor_up(&mut screen, &mut row);
            }
            Key::Down => {
                tui_git.move_cursor_down(&mut screen, &mut row);
            }
            _ => {}
        }
        screen.flush().unwrap();
    }
    write!(screen, "{}", termion::cursor::Show).unwrap();
    screen.flush().unwrap();
}
