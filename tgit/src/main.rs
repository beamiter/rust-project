extern crate termion;

use std::{
    collections::HashMap,
    io::{stdin, stdout, Write},
    process::Command,
    vec,
};
use substring::Substring;

use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::{color, style};
use termion::{cursor::DetectCursorPos, event::Key};

use coredump::register_panic_handler;

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
    branch_row_top: usize,
    branch_row_bottom: usize,
    branch_col_left: usize,
    branch_col_right: usize,

    // log render area;
    log_row_top: usize,
    log_row_bottom: usize,
    log_col_left: usize,
    log_scroll_offset: usize,

    // data storage;
    branch_vec: Vec<String>,
    branch_row_map: HashMap<String, usize>,
    row_branch_map: HashMap<usize, String>,
    branch_log_map: HashMap<String, Vec<String>>,
    row_log_map: HashMap<usize, String>,
    branch_diff_vec: Vec<String>,
    branch_diff_toggle: bool,

    // Main branch;
    main_branch: String,
    current_branch: String,
    current_log_vec: Vec<String>,

    // 0 for title, 1 for branch, 2 for log and e.t.c.
    layout_position: usize,
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
            branch_vec: vec![],
            branch_diff_toggle: false,
            branch_row_map: HashMap::new(),
            row_branch_map: HashMap::new(),
            branch_log_map: HashMap::new(),
            row_log_map: HashMap::new(),
            branch_diff_vec: vec![],
            main_branch: String::new(),
            current_branch: String::new(),
            current_log_vec: vec![],
            layout_position: 0,
            key_move_counter: 0,
        }
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
                if val.starts_with('*') {
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
        self.branch_row_bottom = self.branch_vec.len() as usize + self.branch_row_top - 1;
        self.branch_col_right =
            self.branch_col_left + *branch_size.iter().max().unwrap() as usize + 3;
        self.log_col_left = self.branch_col_right + 3;
    }

    // Currently limit the log number to 100.
    fn update_git_log(&mut self, branch: &String) {
        if self.branch_log_map.get(branch).is_some() {
            return;
        }
        let output = Command::new("git")
            .args(["log", branch.as_str(), "-100"])
            .output()
            .expect("failed to execute process");
        // println!("status: {}", output.status);
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
        self.branch_log_map.insert(branch.to_string(), logs);
    }

    fn update_git_diff(&mut self, branch: &String) {
        let output = Command::new("git")
            .args(["diff", branch.as_str()])
            .output()
            .expect("failed to execute process");
        let diff_output = String::from_utf8_lossy(&output.stdout);
        // println!("branch_output {:?}", diff_output);
        let mut diff_iter = diff_output.split('\n');
        self.branch_diff_vec.clear();
        loop {
            if let Some(val) = diff_iter.next() {
                if val.is_empty() {
                    continue;
                } else {
                    self.branch_diff_vec.push(val.to_string());
                }
            } else {
                break;
            }
        }
        // println!("{:?}", self.branch_diff_vec);
    }
}

trait RenderGit {
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W);
    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W);
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W);

    fn checkout_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn cursor_to_main<W: Write>(&self, screen: &mut W);

    fn update_line_fg<W: Write>(&self, screen: &mut W, log: &String);

    fn refresh_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn enter_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W);

    fn move_cursor_left<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W);

    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool);
    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool);

    fn move_cursor_up<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W);
}

impl RenderGit for TuiGit {
    fn update_line_fg<W: Write>(&self, screen: &mut W, log: &String) {
        if log.is_empty() {
            return;
        }
        if self.branch_diff_toggle {
            // Show "git diff".
            match log.chars().next().unwrap() {
                '-' => {
                    write!(screen, "{}", termion::color::Fg(termion::color::Red)).unwrap();
                }
                '+' => {
                    write!(screen, "{}", termion::color::Fg(termion::color::Green)).unwrap();
                }
                _ => {}
            }
        } else {
            // Show "git log".
            if log.starts_with("commit") {
                write!(screen, "{}", termion::color::Fg(termion::color::Yellow)).unwrap();
            } else {
                write!(screen, "{}", termion::color::Fg(termion::color::Reset)).unwrap();
            }
        }
    }
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W) {
        let (x, y) = screen.cursor_pos().unwrap();
        let (col, row) = termion::terminal_size().unwrap();
        let x_tmp = self.log_col_left;
        if col <= x_tmp as u16 {
            // No show due to no enough col.
            return;
        }
        // Clear previous log zone.
        for clear_y in self.log_row_top..=self.log_row_bottom {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(x_tmp as u16, clear_y as u16),
                termion::clear::UntilNewline,
            )
            .unwrap();
        }
        let mut y_tmp = self.log_row_top;
        self.row_log_map.clear();
        for log in &self.current_log_vec[self.log_scroll_offset as usize..] {
            // Need to update bottom here.
            self.log_row_bottom = y_tmp;
            let sub_log = log.substring(0, (col - x_tmp as u16) as usize);
            if !sub_log.is_empty() {
                self.update_line_fg(screen, log);
                write!(
                    screen,
                    "{}{}{}",
                    termion::cursor::Goto(x_tmp as u16, y_tmp as u16),
                    sub_log,
                    termion::color::Fg(termion::color::Reset),
                )
                .unwrap();
            }
            self.row_log_map.insert(y_tmp, sub_log.to_string());
            // Spare 2 for check info.
            if y_tmp as u16 >= row - 2 {
                break;
            }
            y_tmp += 1;
        }
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
            write!(
                screen,
                "{}{}✅{} Checkout to target branch {}{}{}, enter 'q' to quit{}{}",
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
        }
        output.status.success()
    }

    fn cursor_to_main<W: Write>(&self, screen: &mut W) {
        write!(
            screen,
            "{}🌟",
            termion::cursor::Goto(
                1,
                *self.branch_row_map.get(&self.main_branch).unwrap() as u16
            )
        )
        .unwrap();
    }
    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool) {
        let (_, term_row) = termion::terminal_size().unwrap();
        let (x, mut y) = screen.cursor_pos().unwrap();
        // Clear previous.
        write!(screen, "{} ", termion::cursor::Goto(1, y)).unwrap();
        if up {
            if y > self.branch_row_top as u16 && y <= self.branch_row_bottom as u16 {
                y = y - 1;
            } else {
                y = self.branch_row_bottom as u16;
            }
        } else {
            if y >= self.branch_row_top as u16 && y < self.branch_row_bottom as u16 {
                y = y + 1;
            } else {
                y = self.branch_row_top as u16;
            }
        }
        self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
        write!(
            screen,
            "{}{}c: {}, r: {}, branch: {}, branch_row: {}{}",
            termion::cursor::Goto(1, term_row),
            termion::clear::CurrentLine,
            x,
            y,
            self.current_branch,
            *self.branch_row_map.get(&self.current_branch).unwrap() as u16,
            termion::cursor::Goto(x, y),
        )
        .unwrap();
        screen.flush().unwrap();
        write!(
            screen,
            "{}{}",
            termion::cursor::Goto(1, y),
            UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
        )
        .unwrap();
        // Update current_branch.
        self.current_branch = self.row_branch_map.get(&(y as usize)).unwrap().to_string();
        // Need to reset this!
        self.log_scroll_offset = 0;
        // Show the log.
        self.update_git_log(&self.current_branch.to_string());
        self.current_log_vec = self
            .branch_log_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.show_log_in_right_panel(screen);
    }

    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool) {
        let (_, term_row) = termion::terminal_size().unwrap();
        let (x, mut y) = screen.cursor_pos().unwrap();
        // Clear previous.
        write!(
            screen,
            "{} ",
            termion::cursor::Goto(self.log_col_left as u16 - 2, y)
        )
        .unwrap();
        if up {
            if y == self.log_row_top as u16 {
                // For a close loop browse.
                // *row = self.log_row_bottom;
                // Hit the top.
                if self.log_scroll_offset > 0 {
                    self.log_scroll_offset -= 1;
                    self.show_log_in_right_panel(screen);
                }
            } else {
                y = y - 1;
            }
        } else {
            if y < self.log_row_bottom as u16 {
                y = y + 1;
            } else {
                // For a close loop browse.
                // *row = self.log_row_top;
                // Hit the bottom.
                let log_show_range = self.log_row_bottom - self.log_row_top;
                let current_log_vec_len = self.current_log_vec.len();
                if usize::from(self.log_scroll_offset + log_show_range + 1) < current_log_vec_len {
                    self.log_scroll_offset += 1;
                    self.show_log_in_right_panel(screen);
                }
            }
        }
        write!(
            screen,
            "{}{}c: {}, r: {}, r_bottom: {}, log: {}{}",
            termion::cursor::Goto(1, term_row),
            termion::clear::CurrentLine,
            x,
            y,
            self.log_row_bottom,
            self.row_log_map.get(&(y as usize)).unwrap(),
            termion::cursor::Goto(x, y),
        )
        .unwrap();
        screen.flush().unwrap();
        write!(
            screen,
            "{}{}",
            termion::cursor::Goto(self.log_col_left as u16 - 2, y),
            UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]
        )
        .unwrap();
    }

    // https://symbl.cc/en/
    fn move_cursor_up<W: Write>(&mut self, screen: &mut W) {
        match self.layout_position {
            1 => {
                self.left_panel_handler(screen, true);
            }
            2 => {
                self.right_panel_handler(screen, true);
            }
            _ => {}
        }
    }
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W) {
        match self.layout_position {
            1 => {
                self.left_panel_handler(screen, false);
            }
            2 => {
                self.right_panel_handler(screen, false);
            }
            _ => {}
        }
    }
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W) {
        self.layout_position = 2;
        let (_, y) = screen.cursor_pos().unwrap();

        write!(screen, "{}  ", termion::cursor::Goto(1, y)).unwrap();
        write!(
            screen,
            "{}  {}✍ ",
            termion::cursor::Goto(self.log_col_left as u16 - 2, self.log_row_top as u16),
            termion::cursor::Goto(self.log_col_left as u16 - 2, self.log_row_top as u16)
        )
        .unwrap();
    }
    fn move_cursor_left<W: Write>(&mut self, screen: &mut W) {
        self.layout_position = 1;
        let (x, y) = screen.cursor_pos().unwrap();
        write!(screen, "{}  ", termion::cursor::Goto(x - 2, y)).unwrap();
        write!(
            screen,
            "{}  ",
            termion::cursor::Goto(
                self.log_col_left as u16 - 2,
                *self.branch_row_map.get(&self.current_branch).unwrap() as u16
            )
        )
        .unwrap();
        write!(
            screen,
            "{}❆ ",
            termion::cursor::Goto(
                1,
                *self.branch_row_map.get(&self.current_branch).unwrap() as u16
            )
        )
        .unwrap();
    }

    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W) {
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

    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W) {
        self.layout_position = 1;
        let mut row = 1;
        self.branch_row_map.clear();
        self.row_branch_map.clear();
        for branch in self.branch_vec.to_vec() {
            row += 1;
            if *branch == self.main_branch {
                write!(
                    screen,
                    "{}{}{}{}{}{}{}{} 🐝",
                    termion::cursor::Goto(4, row as u16),
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
                    termion::cursor::Goto(4, row as u16),
                    termion::clear::CurrentLine,
                    branch
                )
                .unwrap();
            }
            self.branch_row_map.insert(branch.to_string(), row);
            self.row_branch_map.insert(row, branch.to_string());
        }
    }

    fn refresh_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String) {
        // Reset with main branch.
        self.current_branch = branch.to_string();
        self.show_title_in_top_panel(screen);
        self.update_git_branch();
        self.show_branch_in_left_panel(screen);
        self.update_git_log(&self.current_branch.to_string());
        self.current_log_vec = self
            .branch_log_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.show_log_in_right_panel(screen);

        self.cursor_to_main(screen);
        screen.flush().unwrap();
    }

    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W) {
        if self.layout_position != 1 {
            return;
        }
        self.log_scroll_offset = 0;
        self.branch_diff_toggle = !self.branch_diff_toggle;
        if self.branch_diff_toggle {
            self.update_git_diff(&self.current_branch.to_string());
            self.current_log_vec = self.branch_diff_vec.to_vec();
            self.show_log_in_right_panel(screen);
        } else {
            self.update_git_log(&self.current_branch.to_string());
            self.current_log_vec = self
                .branch_log_map
                .get(&self.current_branch.to_string())
                .unwrap()
                .to_vec();
            self.show_log_in_right_panel(screen);
        }
    }

    fn enter_pressed<W: Write>(&mut self, screen: &mut W) {
        let (_, y) = screen.cursor_pos().unwrap();
        match self.layout_position {
            1 => {
                let branch =
                    &mut self.branch_vec.to_vec()[(y - self.branch_row_top as u16) as usize];
                if self.checkout_git_branch(screen, branch) {
                } else {
                    *branch = self.main_branch.to_string();
                }
                // Refresh title and branch.
                self.refresh_with_branch(screen, branch);
            }
            2 => {}
            _ => {}
        }
    }
}

fn main() {
    register_panic_handler().unwrap();
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();

    let stdin = stdin();
    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    write!(screen, "{}", termion::cursor::Hide).unwrap();

    tui_git.refresh_with_branch(&mut screen, &tui_git.main_branch.to_string());

    // Start with the main branch row.
    for c in stdin.keys() {
        match c.unwrap() {
            Key::Char('q') => {
                break;
            }
            Key::Char('d') => {
                tui_git.lower_d_pressed(&mut screen);
            }
            Key::Char('D') => {}
            Key::Char('\n') => {
                tui_git.enter_pressed(&mut screen);
            }
            Key::Left => {
                tui_git.move_cursor_left(&mut screen);
            }
            Key::Right => {
                tui_git.move_cursor_right(&mut screen);
            }
            Key::Up => {
                tui_git.move_cursor_up(&mut screen);
            }
            Key::Down => {
                tui_git.move_cursor_down(&mut screen);
            }
            _ => {}
        }
        screen.flush().unwrap();
    }
    write!(screen, "{}", termion::cursor::Show).unwrap();
    screen.flush().unwrap();
}
