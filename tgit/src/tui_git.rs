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
pub enum DisplayType {
    Branch,
    Delete,
    Diff,
    Log,
    Status,
    Commit,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    LeftPanel(DisplayType),
    RightPanel(DisplayType),
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogInfoPattern {
    Author(String),
    Commit(String),
    Date(String),
    DiffAdd(String),
    DiffSubtract(String),
    Msg(String),
    None,
}

// Need to parse commit.
#[derive(Clone, PartialEq, Eq, Debug)]
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
    pub log_col_right: usize,
    pub log_scroll_offset: usize,
    pub log_scroll_offset_max: usize,
    pub snap_shot_map: HashMap<DisplayType, SnapShot>,

    // bottom bar area;
    pub bottom_bar_row: usize,
    // status bar area;
    pub status_bar_row: usize,

    // data storage;
    pub branch_delete_set: HashSet<String>,
    pub branch_diff_vec: Vec<LogInfoPattern>,
    pub branch_log_info_map: HashMap<String, Vec<LogInfoPattern>>,
    pub branch_row_map: HashMap<String, usize>,
    pub branch_vec: Vec<String>,
    pub commit_info_map: HashMap<String, Vec<LogInfoPattern>>,
    pub current_branch: String,
    pub right_panel_log_info: Vec<LogInfoPattern>,
    // Main branch;
    pub main_branch: String,
    pub row_branch_map: HashMap<usize, String>,
    pub row_log_map: HashMap<usize, LogInfoPattern>,

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
            log_col_right: 0,
            log_scroll_offset: 0,
            log_scroll_offset_max: 0,
            snap_shot_map: HashMap::from([
                (DisplayType::Delete, SnapShot::new()),
                (DisplayType::Diff, SnapShot::new()),
                (DisplayType::Log, SnapShot::new()),
                (DisplayType::Status, SnapShot::new()),
                (DisplayType::Commit, SnapShot::new()),
            ]),

            status_bar_row: 0,
            bottom_bar_row: 0,

            branch_delete_set: HashSet::new(),
            branch_diff_vec: vec![],
            branch_log_info_map: HashMap::new(),
            branch_row_map: HashMap::new(),
            branch_vec: vec![],
            commit_info_map: HashMap::new(),
            row_branch_map: HashMap::new(),
            row_log_map: HashMap::new(),

            main_branch: String::new(),
            current_branch: String::new(),
            right_panel_log_info: vec![],
            layout_mode: LayoutMode::LeftPanel(DisplayType::Log),
            key_move_counter: 0,
            // Goto is 1 based.
            previous_pos: Position::init(1, 1),
            current_pos: Position::init(1, 1),
        }
    }

    pub fn update_commit_info(&mut self) -> Option<String> {
        let (_, y) = self.current_pos.unpack();
        let mut current_commit: Option<String> = None;
        if let Some(log) = self.row_log_map.get(&(y as usize)) {
            if *log == LogInfoPattern::None {
                return None;
            }
            if let LogInfoPattern::Commit(val) = log {
                let mut iter = val.split(' ');
                if iter.next().unwrap() != "commit" {
                    return None;
                }
                if let Some(val) = iter.next() {
                    // Find the right commit name.
                    current_commit = Some(val.to_string());
                }
            }
        } else {
            return None;
        }
        if current_commit == None {
            return None;
        }
        let current_commit = current_commit.unwrap();
        if self.commit_info_map.get(&current_commit).is_some() {
            return Some(current_commit);
        }
        let output = Command::new("git")
            .args(["show", &current_commit])
            .output()
            .expect("failed to execute process");
        let commit_output = String::from_utf8_lossy(&output.stdout);
        let mut commit_iter = commit_output.split('\n');
        self.commit_info_map
            .insert(current_commit.to_string(), vec![]);
        loop {
            if let Some(val) = commit_iter.next() {
                if val.is_empty() {
                    continue;
                } else {
                    if val.starts_with("commit") {
                        self.commit_info_map
                            .get_mut(&current_commit)
                            .unwrap()
                            .push(LogInfoPattern::Commit(val.to_string()));
                    } else if val.starts_with("-   ") {
                        self.commit_info_map
                            .get_mut(&current_commit)
                            .unwrap()
                            .push(LogInfoPattern::DiffSubtract(val.to_string()));
                    } else if val.starts_with("+   ") {
                        self.commit_info_map
                            .get_mut(&current_commit)
                            .unwrap()
                            .push(LogInfoPattern::DiffAdd(val.to_string()));
                    } else {
                        self.commit_info_map
                            .get_mut(&current_commit)
                            .unwrap()
                            .push(LogInfoPattern::Msg(val.to_string()));
                    }
                }
            } else {
                break;
            }
        }
        return Some(current_commit.to_string());
    }

    pub fn update_git_branch(&mut self) -> bool {
        let output = Command::new("git").arg("branch").output();
        match output {
            Err(output) => {
                println!("{}", output.to_string());
                return false;
            }
            Ok(output) => {
                if !output.status.success() {
                    println!("{}", String::from_utf8_lossy(&output.stderr).to_string());
                    return false;
                }
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
                            self.main_branch = val.strip_prefix("*").unwrap().trim().to_string();
                            let head_str: &str = "HEAD detached at ";
                            if let Some(pos) = self.main_branch.find(head_str) {
                                self.main_branch = self
                                    .main_branch
                                    .substring(pos + head_str.len(), self.main_branch.len() - 1)
                                    .to_string();
                            }
                            // println!("Main branch: {}", self.main_branch);
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
        }
    }
    pub fn update_git_branch_async(&mut self) -> bool {
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
                    self.main_branch = val.strip_prefix("*").unwrap().trim().to_string();
                    let head_str: &str = "HEAD detached at ";
                    if let Some(pos) = self.main_branch.find(head_str) {
                        self.main_branch = self
                            .main_branch
                            .substring(pos + head_str.len(), self.main_branch.len() - 1)
                            .to_string();
                    }
                    self.branch_vec.push(self.main_branch.to_string());
                    self.update_git_log_async(&self.branch_vec.last().unwrap().to_string());
                } else {
                    self.branch_vec.push(val.to_string().trim().to_string());
                    self.update_git_log_async(&self.branch_vec.last().unwrap().to_string());
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
                    if val.starts_with("-") {
                        self.branch_diff_vec
                            .push(LogInfoPattern::DiffSubtract(val.to_string()));
                    } else if val.starts_with("+") {
                        self.branch_diff_vec
                            .push(LogInfoPattern::DiffAdd(val.to_string()));
                    } else {
                        self.branch_diff_vec
                            .push(LogInfoPattern::Msg(val.to_string()));
                    }
                }
            } else {
                break;
            }
        }
        output.status.success()
    }

    // Currently limit the log number to 100.
    pub fn update_git_log(&mut self, branch: &String) -> bool {
        if self.branch_log_info_map.get(branch).is_some() {
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
        self.branch_log_info_map.insert(branch.to_string(), vec![]);
        // println!("status: {}", output.status);
        // write!(stdout(), "{:?}", String::from_utf8_lossy(&output.stdout)).unwrap();
        let log_output = String::from_utf8_lossy(&output.stdout);
        let mut log_iter = log_output.split('\n');
        loop {
            if let Some(val) = log_iter.next() {
                if val.starts_with("commit") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Commit(val.to_string()));
                } else if val.starts_with("Author") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Author(val.to_string()));
                } else if val.starts_with("Date") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Date(val.to_string()));
                } else if !val.is_empty() {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Msg(val.to_string()));
                } else {
                    // Only keep the last empty row.
                    if let LogInfoPattern::Msg(_) = self
                        .branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .last()
                        .unwrap()
                    {
                        self.branch_log_info_map
                            .get_mut(&branch.to_string())
                            .unwrap()
                            .push(LogInfoPattern::None);
                    }
                }
            } else {
                break;
            }
        }
        output.status.success()
    }
    pub fn update_git_log_async(&mut self, branch: &String) -> bool {
        let output = Command::new("git")
            .args([
                "log",
                "--decorate",
                "--abbrev-commit",
                branch.as_str(),
                "-n 200",
            ])
            .output()
            .expect("failed to execute process");
        self.branch_log_info_map.insert(branch.to_string(), vec![]);
        let log_output = String::from_utf8_lossy(&output.stdout);
        let mut log_iter = log_output.split('\n');
        loop {
            if let Some(val) = log_iter.next() {
                if val.starts_with("commit") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Commit(val.to_string()));
                } else if val.starts_with("Author") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Author(val.to_string()));
                } else if val.starts_with("Date") {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Date(val.to_string()));
                } else if !val.is_empty() {
                    self.branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .push(LogInfoPattern::Msg(val.to_string()));
                } else {
                    // Only keep the last empty row.
                    if let LogInfoPattern::Msg(_) = self
                        .branch_log_info_map
                        .get_mut(&branch.to_string())
                        .unwrap()
                        .last()
                        .unwrap()
                    {
                        self.branch_log_info_map
                            .get_mut(&branch.to_string())
                            .unwrap()
                            .push(LogInfoPattern::None);
                    }
                }
            } else {
                break;
            }
        }
        output.status.success()
    }
}
