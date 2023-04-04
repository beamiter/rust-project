extern crate termion;
use crate::render_git::*;
use crate::tui_git::*;
use crate::event_git::*;

use std::io::Write;

pub trait ActionGit {
    fn enter_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_y_pressed<W: Write>(&mut self, screen: &mut W);
    fn lower_n_pressed<W: Write>(&mut self, screen: &mut W);

    fn upper_d_pressed<W: Write>(&mut self, screen: &mut W);

    fn move_cursor_left<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_right<W: Write>(&mut self, screen: &mut W);

    fn move_cursor_up<W: Write>(&mut self, screen: &mut W);
    fn move_cursor_down<W: Write>(&mut self, screen: &mut W);
}
impl ActionGit for TuiGit {
    // https://symbl.cc/en/
    fn move_cursor_up<W: Write>(&mut self, screen: &mut W) {
        match self.layout_position {
            1 => {
                if self.branch_diff_toggle {
                    return;
                }
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
                if self.branch_diff_toggle {
                    return;
                }
                self.left_panel_handler(screen, false);
            }
            2 => {
                self.right_panel_handler(screen, false);
            }
            _ => {}
        }
    }
    fn move_cursor_left<W: Write>(&mut self, screen: &mut W) {
        if self.layout_position == 1 {
            return;
        }
        self.layout_position = 1;
        // Must update position.
        self.previous_pos = self.current_pos;
        self.current_pos = Position::init(
            1,
            *self.branch_row_map.get(&self.current_branch).unwrap() as u16,
        );
        self.show_icon_after_cursor(screen, "❆");
    }

    fn move_cursor_right<W: Write>(&mut self, screen: &mut W) {
        if self.layout_position == 2 {
            return;
        }
        if !self.branch_delete_set.is_empty() {
            return;
        }
        self.layout_position = 2;
        // Must update position.
        self.previous_pos = self.current_pos;
        self.current_pos = Position::init(self.log_col_left as u16 - 3, self.log_row_top as u16);
        self.show_icon_after_cursor(screen, "✍");
    }
    fn upper_d_pressed<W: Write>(&mut self, screen: &mut W) {
        if self.layout_position != 1 {
            return;
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
    }
    fn lower_y_pressed<W: Write>(&mut self, screen: &mut W) {
        if !self.branch_delete_set.is_empty() {
            self.delete_git_branch(screen);
        }
    }
    fn lower_n_pressed<W: Write>(&mut self, screen: &mut W) {
        if !self.branch_delete_set.is_empty() {
            self.show_in_status_bar(screen, &format!("Escape deleting branch").to_string());
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
        }
    }
    fn enter_pressed<W: Write>(&mut self, screen: &mut W) {
        match self.layout_position {
            1 => {
                if self.branch_delete_set.is_empty() {
                    self.checkout_local_git_branch(screen, &self.current_branch.to_string());
                }
            }
            2 => {}
            _ => {}
        }
    }
    fn lower_d_pressed<W: Write>(&mut self, screen: &mut W) {
        self.log_scroll_offset = 0;
        self.branch_diff_toggle = !self.branch_diff_toggle;
        if self.branch_diff_toggle {
            self.update_git_diff(&self.current_branch.to_string());
            self.current_log_vec = self.branch_diff_vec.to_vec();
            self.show_log_in_right_panel(screen);
            if self.layout_position == 1 {
                self.move_cursor_right(screen);
            }
        } else {
            self.update_git_log(&self.current_branch.to_string());
            self.current_log_vec = self
                .branch_log_map
                .get(&self.current_branch.to_string())
                .unwrap()
                .to_vec();
            self.show_log_in_right_panel(screen);
            if self.layout_position == 2 {
                self.move_cursor_left(screen);
            }
        }
    }
}
