use std::collections::{HashMap, HashSet};
use std::process::Command;
use substring::Substring;

// https://en.wikipedia.org/wiki/ANSI_escape_code
// https://symbl.cc/en/
pub const UNICODE_TABLE: [&'static str; 12] = [
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub col: u16,
    pub row: u16,
}
impl Position {
    pub fn new() -> Position {
        Position { col: 0, row: 0 }
    }
    pub fn init(c: u16, r: u16) -> Position {
        Position { col: c, row: r }
    }
    pub fn unpack(self) -> (u16, u16) {
        return (self.col, self.row);
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Delete,
    Diff,
    Log,
    Status,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    LeftPanel(ContentType),
    RightPanel(ContentType),
}

pub struct TuiGit {
    // branch render area;
    pub branch_row_top: usize,
    pub branch_row_bottom: usize,
    pub branch_col_left: usize,
    pub branch_col_right: usize,

    // log render area;
    pub log_row_top: usize,
    pub log_row_bottom: usize,
    pub log_col_left: usize,
    pub log_scroll_offset: usize,

    // status bar area;
    pub status_bar_row: usize,
    pub bottom_bar_row: usize,

    // data storage;
    pub branch_vec: Vec<String>,
    pub branch_row_map: HashMap<String, usize>,
    pub row_branch_map: HashMap<usize, String>,
    pub branch_log_map: HashMap<String, Vec<String>>,
    pub row_log_map: HashMap<usize, String>,
    pub branch_diff_vec: Vec<String>,
    pub branch_delete_set: HashSet<String>,

    // Main branch;
    pub main_branch: String,
    pub current_branch: String,
    pub current_log_vec: Vec<String>,

    // 0 for title, 1 for branch, 2 for log and e.t.c.
    pub layout_mode: LayoutMode,
    pub key_move_counter: usize,

    pub previous_pos: Position,
    pub current_pos: Position,
}
impl TuiGit {
    pub fn new() -> TuiGit {
        TuiGit {
            branch_row_top: 2,
            branch_row_bottom: 0,
            branch_col_left: 4,
            branch_col_right: 0,
            log_row_top: 2,
            log_row_bottom: 0,
            log_col_left: 0,
            log_scroll_offset: 0,
            status_bar_row: 0,
            bottom_bar_row: 0,
            branch_vec: vec![],
            branch_delete_set: HashSet::new(),
            branch_row_map: HashMap::new(),
            row_branch_map: HashMap::new(),
            branch_log_map: HashMap::new(),
            row_log_map: HashMap::new(),
            branch_diff_vec: vec![],
            main_branch: String::new(),
            current_branch: String::new(),
            current_log_vec: vec![],
            layout_mode: LayoutMode::LeftPanel(ContentType::Log),
            key_move_counter: 0,
            previous_pos: Position::new(),
            current_pos: Position::new(),
        }
    }
    pub fn update_git_branch(&mut self) {
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
    }

    // Currently limit the log number to 100.
    pub fn update_git_log(&mut self, branch: &String) {
        if self.branch_log_map.get(branch).is_some() {
            return;
        }
        let output = Command::new("git")
            .args([
                "log",
                "--decorate",
                "--abbrev-commit",
                branch.as_str(),
                "-n 100",
            ])
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

    pub fn update_git_diff(&mut self, branch: &String) {
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
