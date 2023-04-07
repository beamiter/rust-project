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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContentType {
    Branch,
    Delete,
    Diff,
    Log,
    Status,
    Commit,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    LeftPanel(ContentType),
    RightPanel(ContentType),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapShot {
    pub position: Position,
    pub scroll_offset: usize,
}
impl SnapShot {
    pub fn new() -> Self {
        Self {
            position: Position::new(),
            scroll_offset: 0,
        }
    }
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
    pub snap_shot_map: HashMap<ContentType, SnapShot>,

    // bottom bar area;
    pub bottom_bar_row: usize,
    // status bar area;
    pub status_bar_row: usize,

    // data storage;
    pub branch_delete_set: HashSet<String>,
    pub branch_diff_vec: Vec<String>,
    pub branch_log_map: HashMap<String, Vec<String>>,
    pub branch_row_map: HashMap<String, usize>,
    pub branch_vec: Vec<String>,
    pub commit_info_map: HashMap<String, Vec<String>>,
    pub current_branch: String,
    pub current_commit: String,
    pub right_panel_log_vec: Vec<String>,
    // Main branch;
    pub main_branch: String,
    pub row_branch_map: HashMap<usize, String>,
    pub row_log_map: HashMap<usize, String>,

    // layout mode;
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
            snap_shot_map: HashMap::from([
                (ContentType::Delete, SnapShot::new()),
                (ContentType::Diff, SnapShot::new()),
                (ContentType::Log, SnapShot::new()),
                (ContentType::Status, SnapShot::new()),
                (ContentType::Commit, SnapShot::new()),
            ]),

            status_bar_row: 0,
            bottom_bar_row: 0,

            branch_delete_set: HashSet::new(),
            branch_diff_vec: vec![],
            branch_log_map: HashMap::new(),
            branch_row_map: HashMap::new(),
            branch_vec: vec![],
            commit_info_map: HashMap::new(),
            row_branch_map: HashMap::new(),
            row_log_map: HashMap::new(),

            main_branch: String::new(),
            current_branch: String::new(),
            current_commit: String::new(),
            right_panel_log_vec: vec![],
            layout_mode: LayoutMode::LeftPanel(ContentType::Log),
            key_move_counter: 0,
            // Goto is 1 based.
            previous_pos: Position::init(1, 1),
            current_pos: Position::init(1, 1),
        }
    }

    pub fn update_commit_info(&mut self) -> bool {
        let (_, y) = self.current_pos.unpack();
        self.current_commit.clear();
        if let Some(log) = self.row_log_map.get(&(y as usize)) {
            if log.is_empty() {
                return false;
            }
            let mut log_iter = log.split(' ');
            if log_iter.next().unwrap() != "commit" {
                return false;
            }
            if let Some(val) = log_iter.next() {
                // Find the right commit name.
                self.current_commit = val.to_string();
            }
        } else {
            return false;
        }
        if self.current_commit.is_empty() {
            return false;
        }
        if self.commit_info_map.get(&self.current_commit).is_some() {
            return true;
        }
        let output = Command::new("git")
            .args(["show", self.current_commit.as_str()])
            .output()
            .expect("failed to execute process");
        let commit_output = String::from_utf8_lossy(&output.stdout);
        let mut commit_iter = commit_output.split('\n');
        let mut commit_detail: Vec<String> = vec![];
        loop {
            if let Some(val) = commit_iter.next() {
                if val.is_empty() {
                    continue;
                } else {
                    commit_detail.push(val.to_string());
                }
            } else {
                break;
            }
        }
        self.commit_info_map
            .insert(self.current_commit.to_string(), commit_detail);
        return true;
    }

    pub fn update_git_branch(&mut self) -> bool {
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
        output.status.success()
    }

    pub fn update_git_diff(&mut self, branch: &String) -> bool {
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
        output.status.success()
    }

    // Currently limit the log number to 100.
    pub fn update_git_log(&mut self, branch: &String) -> bool {
        if self.branch_log_map.get(branch).is_some() {
            return true;
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
        output.status.success()
    }
}
