extern crate termion;
use crate::event_git::*;
use crate::render_git::*;
use crate::tui_git::*;

use std::{
    io::{stdin, Read, Write},
    str, vec,
};

pub trait ActionGit {
    // Lower case.
    fn lower_b_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_c_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_f_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_n_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_q_pressed<W: Write>(&mut self, screen: &mut W) -> bool;
    fn lower_y_pressed<W: Write>(&mut self, screen: &mut W);

    // Upper case.
    fn upper_d_pressed<W: Write>(&mut self, screen: &mut W);

    // Special character.
    fn colon_pressed<W: Write>(&mut self, screen: &mut W);
    fn enter_pressed<W: Write>(&mut self, screen: &mut W);

    // Cursor.
    fn move_cursor_left<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_up<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W);
}
// https://symbl.cc/en/
impl ActionGit for TuiGit {
    fn lower_b_pressed<W: Write>(&mut self, screen: &mut W) {
        // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
        let mut bufs = vec![];
        let mut buffer: &str = "";
        self.show_and_stay_in_status_bar(screen, &"git checkout -b: ".to_string());
        loop {
            let b = stdin().lock().bytes().next().unwrap().unwrap();
            match char::from(b) {
                '\r' | '\n' => {
                    self.checkout_new_git_branch(screen, &buffer.to_string());
                    break;
                }
                // Backspace '\b'
                '\x7f' => {
                    if !bufs.is_empty() {
                        bufs.remove(bufs.len() - 1);
                    }
                }
                _ => {
                    bufs.push(b);
                }
            }
            buffer = str::from_utf8(&bufs).unwrap();
            self.show_and_stay_in_status_bar(
                screen,
                &format!("git checkout -b: {}", buffer.to_string()).to_string(),
            );
        }
    }
    fn lower_c_pressed<W: Write>(&mut self, screen: &mut W) {
        // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
        let mut bufs = vec![];
        let mut buffer: &str = "";
        self.show_and_stay_in_status_bar(screen, &"git checkout: ".to_string());
        loop {
            let b = stdin().lock().bytes().next().unwrap().unwrap();
            match char::from(b) {
                '\r' | '\n' => {
                    self.checkout_local_git_branch(screen, &buffer.to_string());
                    break;
                }
                // Backspace '\b'
                '\x7f' => {
                    if !bufs.is_empty() {
                        bufs.remove(bufs.len() - 1);
                    }
                }
                _ => {
                    bufs.push(b);
                }
            }
            buffer = str::from_utf8(&bufs).unwrap();
            self.show_and_stay_in_status_bar(
                screen,
                &format!("git checkout: {}", buffer.to_string()).to_string(),
            );
        }
    }
    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(content) => {
                self.log_scroll_offset = 0;
                match content {
                    DisplayType::Diff => {
                        self.layout_mode = LayoutMode::LeftPanel(DisplayType::Log);
                        self.update_git_log(&self.current_branch.to_string());
                        self.right_panel_log_info = self
                            .branch_log_info_map
                            .get(&self.current_branch.to_string())
                            .unwrap()
                            .to_vec();
                        self.show_log_in_right_panel(screen);
                    }
                    DisplayType::Log => {
                        self.layout_mode = LayoutMode::LeftPanel(DisplayType::Diff);
                        self.update_git_diff(&self.current_branch.to_string());
                        self.right_panel_log_info = self.branch_diff_vec.clone();
                        self.show_log_in_right_panel(screen);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
    }
    fn lower_f_pressed<W: Write>(&mut self, screen: &mut W) {
        // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
        let mut bufs = vec![];
        let mut buffer: &str = "";
        self.show_and_stay_in_status_bar(screen, &"git fetch and check: ".to_string());
        loop {
            let b = stdin().lock().bytes().next().unwrap().unwrap();
            match char::from(b) {
                '\r' | '\n' => {
                    self.checkout_remote_git_branch(screen, &buffer.to_string());
                    break;
                }
                // Backspace '\b'
                '\x7f' => {
                    if !bufs.is_empty() {
                        bufs.remove(bufs.len() - 1);
                    }
                }
                _ => {
                    bufs.push(b);
                }
            }
            buffer = str::from_utf8(&bufs).unwrap();
            self.show_and_stay_in_status_bar(
                screen,
                &format!("get fetch and check: {}", buffer.to_string()).to_string(),
            );
        }
    }
    fn lower_n_pressed<W: Write>(&mut self, screen: &mut W) {
        if let LayoutMode::LeftPanel(DisplayType::Delete) = self.layout_mode {
            // Reset chosen branch background.
            for branch in &self.branch_delete_set {
                let y = self.branch_row_map.get(branch).unwrap();
                write!(
                    screen,
                    "{}{}{}",
                    termion::color::Bg(termion::color::Reset),
                    termion::cursor::Goto(self.branch_col_left as u16, *y as u16),
                    branch,
                )
                .unwrap();
            }
            self.branch_delete_set.clear();
            self.show_in_status_bar(screen, &format!("Escape deleting branch").to_string());
            // Reset layout_mode.
            self.layout_mode = LayoutMode::LeftPanel(DisplayType::Log);
            // Refresh frame.
            self.refresh_frame_with_branch(screen, &self.current_branch.to_string());
            self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
        }
    }
    fn lower_q_pressed<W: Write>(&mut self, _: &mut W) -> bool {
        let quit: bool = true;
        return quit;
    }
    fn lower_y_pressed<W: Write>(&mut self, screen: &mut W) {
        if let LayoutMode::LeftPanel(DisplayType::Delete) = self.layout_mode {
            if self.delete_git_branch(screen) {
                // Reset layout_mode.
                self.layout_mode = LayoutMode::LeftPanel(DisplayType::Log);
                // Refresh frame.
                self.refresh_frame_with_branch(screen, &self.main_branch.to_string());
                self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
            }
        }
    }
    fn upper_d_pressed<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(_) => {
                self.layout_mode = LayoutMode::LeftPanel(DisplayType::Delete);
            }
            _ => {
                return;
            }
        }
        if self.current_branch == self.main_branch {
            self.show_in_status_bar(
                screen,
                &format!(
                    "{}Cann't delete current branch you are in!{}",
                    termion::color::Fg(termion::color::LightRed),
                    termion::color::Fg(termion::color::Reset),
                )
                .to_string(),
            );
            return;
        }
        self.show_in_status_bar(
            screen,
            &"Press 'y' to confirm delete, 'n' to escape\n".to_string(),
        );
        let branch = self.current_branch.to_string();
        // Toggle branch delete.
        if self.branch_delete_set.get(&branch).is_some() {
            self.branch_delete_set.remove(&branch);
            let y = self.branch_row_map.get(&branch).unwrap();
            write!(
                screen,
                "{}{}{}{}",
                termion::color::Bg(termion::color::Reset),
                termion::cursor::Goto(self.branch_col_left as u16, *y as u16),
                branch,
                termion::color::Bg(termion::color::Reset),
            )
            .unwrap();
        } else {
            self.branch_delete_set.insert(branch.to_string());
            let y = self.branch_row_map.get(&branch).unwrap();
            write!(
                screen,
                "{}{}{}{}",
                termion::color::Bg(termion::color::Red),
                termion::cursor::Goto(self.branch_col_left as u16, *y as u16),
                branch,
                termion::color::Bg(termion::color::Reset),
            )
            .unwrap();
        }
        self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
    }
    fn colon_pressed<W: Write>(&mut self, screen: &mut W) {
        // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
        let mut bufs = vec![];
        let mut buffer: &str = "";
        self.show_and_stay_in_status_bar(screen, &":".to_string());
        for b in stdin().lock().bytes() {
            match b {
                Ok(b'\r') | Ok(b'\n') => {
                    self.execute_normal_command(screen, &buffer);
                    break;
                }
                // Backspace '\b'
                // '\u{1b}' or '\x1b' for escape
                Ok(0x7f) => {
                    bufs.pop();
                }
                Ok(c) => {
                    bufs.push(c);
                }
                Err(_) => {}
            }
            buffer = str::from_utf8(&bufs).unwrap();
            self.show_and_stay_in_status_bar(
                screen,
                &format!(":{}", buffer.to_string()).to_string(),
            );
        }
    }
    fn enter_pressed<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(content) => match content {
                DisplayType::Log => {
                    self.checkout_local_git_branch(screen, &self.current_branch.to_string());
                }
                _ => {}
            },
            LayoutMode::RightPanel(content) => match content {
                DisplayType::Log => {
                    let current_commit = self.update_commit_info();
                    if current_commit.is_none() {
                        return;
                    }
                    self.reset_cursor_to_log_top(screen);
                    self.log_scroll_offset = 0;
                    self.layout_mode = LayoutMode::RightPanel(DisplayType::Commit);
                    self.right_panel_log_info = self
                        .commit_info_map
                        .get(&current_commit.unwrap())
                        .unwrap()
                        .to_vec();
                    self.show_log_in_right_panel(screen);
                }
                DisplayType::Commit => {
                    // Update with log position and scroll offset.
                    self.log_scroll_offset = self
                        .snap_shot_map
                        .get(&DisplayType::Log)
                        .unwrap()
                        .scroll_offset;
                    self.previous_pos = self.current_pos;
                    self.current_pos = self.snap_shot_map.get(&DisplayType::Log).unwrap().position;
                    self.layout_mode = LayoutMode::RightPanel(DisplayType::Log);
                    self.update_git_log(&self.current_branch.to_string());
                    self.right_panel_log_info = self
                        .branch_log_info_map
                        .get(&self.current_branch.to_string())
                        .unwrap()
                        .to_vec();
                    self.show_log_in_right_panel(screen);
                }
                _ => {}
            },
        }
        self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
    }
    fn move_cursor_up<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(content) => {
                match content {
                    DisplayType::Diff => {
                        return;
                    }
                    _ => {
                        self.left_panel_handler(screen, true);
                    }
                }
                self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
            }
            LayoutMode::RightPanel(_) => {
                // self.show_branch_in_left_panel(screen);
                self.right_panel_handler(screen, true);
            }
        }
        // Update snapshot.
        match self.layout_mode {
            LayoutMode::LeftPanel(content) | LayoutMode::RightPanel(content) => {
                let mut snap_shot = SnapShot::new();
                snap_shot.scroll_offset = self.log_scroll_offset;
                snap_shot.position = self.current_pos;
                self.snap_shot_map.insert(content, snap_shot);
            }
        }
    }

    fn move_cursor_down<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(content) => {
                match content {
                    DisplayType::Diff => {
                        return;
                    }
                    _ => {
                        self.left_panel_handler(screen, false);
                    }
                }
                self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
            }
            LayoutMode::RightPanel(_) => {
                // self.show_branch_in_left_panel(screen);
                self.right_panel_handler(screen, false);
            }
        }
        // Update snapshot.
        match self.layout_mode {
            LayoutMode::LeftPanel(content) | LayoutMode::RightPanel(content) => {
                let mut snap_shot = SnapShot::new();
                snap_shot.scroll_offset = self.log_scroll_offset;
                snap_shot.position = self.current_pos;
                self.snap_shot_map.insert(content, snap_shot);
            }
        }
    }
    fn move_cursor_left<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(_) => {
                return;
            }
            LayoutMode::RightPanel(content) => match content {
                DisplayType::Log | DisplayType::Diff => {
                    self.layout_mode = LayoutMode::LeftPanel(content);
                }
                _ => {
                    return;
                }
            },
        }
        self.reset_cursor_to_current_branch(screen);
        self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
        // Update snapshot.
        match self.layout_mode {
            LayoutMode::LeftPanel(content) | LayoutMode::RightPanel(content) => {
                let mut snap_shot = SnapShot::new();
                snap_shot.scroll_offset = self.log_scroll_offset;
                snap_shot.position = self.current_pos;
                self.snap_shot_map.insert(content, snap_shot);
            }
        }
    }

    fn move_cursor_right<W: Write>(&mut self, screen: &mut W) {
        match self.layout_mode {
            LayoutMode::LeftPanel(content) => match content {
                DisplayType::Delete => {
                    return;
                }
                _ => {
                    self.layout_mode = LayoutMode::RightPanel(content);
                }
            },
            LayoutMode::RightPanel(_) => {
                return;
            }
        }
        self.reset_cursor_to_log_top(screen);
        self.show_in_bottom_bar(screen, &format!("{:?}", self.layout_mode).to_string());
        // Update snapshot.
        match self.layout_mode {
            LayoutMode::LeftPanel(content) | LayoutMode::RightPanel(content) => {
                let mut snap_shot = SnapShot::new();
                snap_shot.scroll_offset = self.log_scroll_offset;
                snap_shot.position = self.current_pos;
                self.snap_shot_map.insert(content, snap_shot);
            }
        }
    }
}
