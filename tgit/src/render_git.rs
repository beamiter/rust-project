extern crate termion;
use crate::tui_git::*;

use std::io::Write;
use std::str;
use substring::Substring;

use termion::cursor::DetectCursorPos;
use termion::{color, style};

pub trait RenderGit {
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W);
    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W);
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W);

    fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    fn show_and_stay_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    fn show_in_bottom_bar<W: Write>(&mut self, screen: &mut W, log: &String);

    fn reset_cursor_to_main<W: Write>(&mut self, screen: &mut W);

    fn render_single_line<W: Write>(&self, screen: &mut W, log: &String, x_tmp: u16, y_tmp: u16);

    fn refresh_frame_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool);
    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool);

    fn show_current_cursor<W: Write>(&mut self, screen: &mut W);
    fn show_icon_after_cursor<W: Write>(&mut self, screen: &mut W, icon: &str);
}

impl RenderGit for TuiGit {
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W) {
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
        let (col, row) = termion::terminal_size().unwrap();
        let x_tmp = self.branch_col_left;
        if col <= x_tmp as u16 {
            // No show due to no enough col.
            return;
        }
        // Clear previous branch zone.
        for clear_y in self.branch_row_top..=self.branch_row_bottom {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(x_tmp as u16, clear_y as u16),
                termion::clear::CurrentLine,
            )
            .unwrap();
        }
        let mut y_tmp = self.branch_row_top;
        self.branch_row_map.clear();
        self.row_branch_map.clear();
        for branch in self.branch_vec.to_vec() {
            // Need to update bottom here.
            self.branch_row_bottom = y_tmp;
            if *branch == self.main_branch {
                write!(
                    screen,
                    "{}{}{}{}{}{}{}{} 🐝",
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
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
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
                    termion::clear::CurrentLine,
                    branch
                )
                .unwrap();
            }
            self.branch_row_map.insert(branch.to_string(), y_tmp);
            self.row_branch_map.insert(y_tmp, branch.to_string());
            // Spare 2 for check info.
            if y_tmp as u16 >= row - 2 {
                break;
            }
            y_tmp += 1;
        }
        let branch_size = self
            .branch_vec
            .iter()
            .map(|x| x.len())
            .collect::<Vec<usize>>();
        self.branch_col_right =
            self.branch_col_left + *branch_size.iter().max().unwrap() as usize + 3;
        self.log_col_left = self.branch_col_right + 4;
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
        for log in &self.right_panel_log_vec[self.log_scroll_offset as usize..] {
            // Need to update bottom here.
            self.log_row_bottom = y_tmp;
            let sub_log = log.substring(0, (col - x_tmp as u16) as usize);
            if !sub_log.is_empty() {
                self.render_single_line(screen, &sub_log.to_string(), x_tmp as u16, y_tmp as u16);
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
    fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
        let (x, y) = screen.cursor_pos().unwrap();
        let (col, row) = termion::terminal_size().unwrap();
        self.status_bar_row = row as usize - 1;
        write!(
            screen,
            "{}{}{}",
            termion::cursor::Goto(1, self.status_bar_row as u16),
            termion::clear::CurrentLine,
            color::Fg(color::LightYellow),
        )
        .unwrap();
        write!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
        write!(
            screen,
            "{}{}",
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y)
        )
        .unwrap();
        screen.flush().unwrap();
    }
    fn show_and_stay_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
        let (col, row) = termion::terminal_size().unwrap();
        self.status_bar_row = row as usize - 1;
        write!(
            screen,
            "{}{}{}",
            termion::cursor::Goto(1, self.status_bar_row as u16),
            termion::clear::CurrentLine,
            color::Fg(color::LightYellow),
        )
        .unwrap();
        write!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
        screen.flush().unwrap();
    }
    fn show_in_bottom_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
        let (x, y) = screen.cursor_pos().unwrap();
        let (col, row) = termion::terminal_size().unwrap();
        self.bottom_bar_row = row as usize;
        write!(
            screen,
            "{}{}{}",
            termion::cursor::Goto(1, self.bottom_bar_row as u16),
            termion::clear::CurrentLine,
            color::Fg(color::Yellow),
        )
        .unwrap();
        write!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
        write!(
            screen,
            "{}{}",
            color::Fg(color::Reset),
            termion::cursor::Goto(x, y)
        )
        .unwrap();
        screen.flush().unwrap();
    }

    fn reset_cursor_to_main<W: Write>(&mut self, screen: &mut W) {
        self.current_pos = Position {
            col: 1,
            row: *self.branch_row_map.get(&self.main_branch).unwrap() as u16,
        };
        self.previous_pos = self.current_pos;
        self.show_icon_after_cursor(screen, "🌟");
    }

    fn render_single_line<W: Write>(&self, screen: &mut W, log: &String, x_tmp: u16, y_tmp: u16) {
        if log.is_empty() {
            return;
        }
        if let LayoutMode::LeftPanel(ContentType::Diff)
        | LayoutMode::RightPanel(ContentType::Diff) = self.layout_mode
        {
            // Show "git diff".
            match log.chars().next().unwrap() {
                '-' => {
                    write!(screen, "{}", termion::color::Fg(termion::color::LightRed)).unwrap();
                }
                '+' => {
                    write!(screen, "{}", termion::color::Fg(termion::color::LightGreen)).unwrap();
                }
                _ => {}
            }
        } else {
            // Show "git log".
            if log.starts_with("commit") {
                write!(
                    screen,
                    "{}",
                    termion::color::Fg(termion::color::LightYellow)
                )
                .unwrap();
                let split_log: Vec<_> = log.split('(').collect();
                if split_log.len() == 2 {
                    write!(
                        screen,
                        "{}{}{}",
                        termion::cursor::Goto(x_tmp as u16, y_tmp as u16),
                        split_log.first().unwrap(),
                        termion::color::Fg(termion::color::Reset),
                    )
                    .unwrap();
                    let split_last: String = split_log
                        .last()
                        .unwrap()
                        .chars()
                        .filter(|&x| x != ')' && x != '(')
                        .collect::<String>();
                    let decoration_split: Vec<_> = split_last.split(", ").collect();
                    let mut decoration_out: String = format!("{}(", style::Bold);
                    for decoration in decoration_split {
                        if decoration.starts_with("HEAD -> ") {
                            decoration_out += format!(
                                "{}HEAD -> {}{}{}",
                                termion::color::Fg(termion::color::LightCyan),
                                termion::color::Fg(termion::color::LightGreen),
                                decoration.strip_prefix("HEAD -> ").unwrap(),
                                termion::color::Fg(termion::color::Reset),
                            )
                            .as_str();
                        } else if decoration.starts_with("origin/") {
                            decoration_out += format!(
                                "{}{}{}",
                                termion::color::Fg(termion::color::LightRed),
                                decoration,
                                termion::color::Fg(termion::color::Reset),
                            )
                            .as_str();
                        } else {
                            decoration_out += format!(
                                "{}{}{}",
                                termion::color::Fg(termion::color::LightGreen),
                                decoration,
                                termion::color::Fg(termion::color::Reset),
                            )
                            .as_str();
                        }
                        decoration_out += ", ";
                    }
                    if decoration_out.ends_with(", ") {
                        decoration_out = decoration_out.strip_suffix(", ").unwrap().to_string();
                    }
                    decoration_out += format!("){}", style::Reset).as_str();
                    write!(
                        screen,
                        "{}{}",
                        decoration_out,
                        termion::color::Fg(termion::color::Reset),
                    )
                    .unwrap();
                    return;
                }
            }
        }
        write!(
            screen,
            "{}{}{}",
            termion::cursor::Goto(x_tmp as u16, y_tmp as u16),
            log,
            termion::color::Fg(termion::color::Reset),
        )
        .unwrap();
    }

    fn refresh_frame_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String) {
        // Reset with main branch.
        self.current_branch = branch.to_string();
        self.show_title_in_top_panel(screen);
        self.update_git_branch();
        self.show_branch_in_left_panel(screen);
        self.update_git_log(&self.current_branch.to_string());
        self.right_panel_log_vec = self
            .branch_log_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.show_log_in_right_panel(screen);

        self.reset_cursor_to_main(screen);
        screen.flush().unwrap();
    }
    fn show_current_cursor<W: Write>(&mut self, screen: &mut W) {
        write!(
            screen,
            "{}",
            termion::cursor::Goto(self.current_pos.col, self.current_pos.row),
        )
        .unwrap();
    }
    fn show_icon_after_cursor<W: Write>(&mut self, screen: &mut W, icon: &str) {
        write!(
            screen,
            "{}{}",
            termion::cursor::Goto(self.previous_pos.col, self.previous_pos.row),
            " ".repeat(2),
        )
        .unwrap();
        write!(
            screen,
            "{}{}{}",
            termion::cursor::Goto(self.current_pos.col, self.current_pos.row),
            icon,
            termion::cursor::Goto(self.current_pos.col, self.current_pos.row),
        )
        .unwrap();
    }

    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool) {
        // Reset something here.
        self.previous_pos = self.current_pos;
        self.log_scroll_offset = 0;

        let (x, mut y) = self.current_pos.unpack();
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
        self.current_pos = Position::init(x, y);
        self.show_in_bottom_bar(
            screen,
            &format!(
                "c: {}, r: {}, branch: {}, branch_row: {}",
                self.current_pos.col,
                self.current_pos.row,
                self.current_branch,
                *self.branch_row_map.get(&self.current_branch).unwrap() as u16,
            )
            .to_string(),
        );
        self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
        self.show_icon_after_cursor(
            screen,
            UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()],
        );
        // Update current_branch.
        self.current_branch = self.row_branch_map.get(&(y as usize)).unwrap().to_string();
        // Show the log.
        self.update_git_log(&self.current_branch.to_string());
        self.right_panel_log_vec = self
            .branch_log_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.show_log_in_right_panel(screen);
        screen.flush().unwrap();
    }

    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, up: bool) {
        // Reset something here.
        self.previous_pos = self.current_pos;

        let (x, mut y) = self.current_pos.unpack();
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
                let right_panel_log_vec_len = self.right_panel_log_vec.len();
                if usize::from(self.log_scroll_offset + log_show_range + 1)
                    < right_panel_log_vec_len
                {
                    self.log_scroll_offset += 1;
                    self.show_log_in_right_panel(screen);
                }
            }
        }
        self.current_pos = Position::init(x, y);
        // Comment following to speed up.
        // self.show_in_bottom_bar(
        //     screen,
        //     &format!(
        //         "c: {}, r: {}, r_bottom: {}, log: {}",
        //         self.current_pos.col,
        //         self.current_pos.row,
        //         self.log_row_bottom,
        //         self.row_log_map.get(&(y as usize)).unwrap(),
        //     )
        //     .to_string(),
        // );
        // self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
        // self.show_icon_after_cursor(
        //     screen,
        //     UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()],
        // );
        self.show_current_cursor(screen);
        screen.flush().unwrap();
    }
}
