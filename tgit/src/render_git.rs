extern crate termion;
use crate::tui_git::*;

use std::io::Write;
use std::str;
use substring::Substring;

use termion::{color, style};

pub trait RenderGit {
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W);
    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W);
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W);

    fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    fn show_and_stay_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    fn show_in_bottom_bar<W: Write>(&mut self, screen: &mut W, log: &String);

    fn reset_cursor_to_current_pos<W: Write>(&mut self, screen: &mut W);
    fn reset_cursor_to_current_branch<W: Write>(&mut self, screen: &mut W);
    fn reset_cursor_to_log_top<W: Write>(&mut self, screen: &mut W);
    fn reset_cursor_to_main_branch<W: Write>(&mut self, screen: &mut W);

    fn render_single_line<W: Write>(
        &mut self,
        screen: &mut W,
        log: &LogInfoPattern,
        x_tmp: u16,
        y_tmp: u16,
    );

    fn refresh_frame_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection);
    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection);

    fn show_icon_after_cursor<W: Write>(&mut self, screen: &mut W, icon: &str);
    fn show_icon_after_cursor_and_wipe<W: Write>(&mut self, screen: &mut W, icon: &str);
}

impl RenderGit for TuiGit {
    // https://unix.stackexchange.com/questions/559708/how-to-draw-a-continuous-line-in-terminal
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W) {
        let (col, row) = termion::terminal_size().unwrap();
        write!(
            screen,
            "{}{}{}{}Welcome to tui git{}{}{}{}{}{}{}\n",
            termion::cursor::Goto(19, 1),
            termion::clear::CurrentLine,
            color::Fg(color::Magenta),
            style::Bold,
            style::Italic,
            color::Fg(color::Reset),
            termion::cursor::Goto(1, 2),
            "⎽".repeat(col as usize),
            termion::cursor::Goto(1, row - self.bar_row_height as u16 + 1),
            "⎺".repeat(col as usize),
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
        for clear_y in self.branch_row_top..row as usize - self.bar_row_height {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(1, clear_y as u16),
                termion::clear::CurrentLine,
            )
            .unwrap();
        }
        let mut y_tmp = self.branch_row_top;
        for branch in self.branch_vec.to_vec() {
            // Need to update bottom here.
            self.branch_row_bottom = y_tmp;
            if *branch == self.main_branch && *branch == self.current_branch {
                self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
                write!(
                    screen,
                    "{}{}{}{}{}{}{}{}{} 🐝",
                    termion::cursor::Goto(1, y_tmp as u16),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()],
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
                    color::Fg(color::Green),
                    style::Bold,
                    style::Underline,
                    branch,
                    color::Fg(color::Reset),
                    style::Reset,
                )
                .unwrap();
            } else if *branch == self.main_branch {
                write!(
                    screen,
                    "{}{}{}{}{} 🐝",
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::Reset),
                    style::Reset,
                )
                .unwrap();
            } else if *branch == self.current_branch {
                self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
                write!(
                    screen,
                    "{}{}{}{}{}{}{}",
                    termion::cursor::Goto(1, y_tmp as u16),
                    UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()],
                    style::Bold,
                    style::Underline,
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
                    branch,
                    style::Reset,
                )
                .unwrap();
            } else {
                write!(
                    screen,
                    "{}{}",
                    termion::cursor::Goto(self.branch_col_left as u16, y_tmp as u16),
                    branch
                )
                .unwrap();
            }
            // Spare 2 for check info.
            if y_tmp as u16 >= row - self.bar_row_height as u16 {
                break;
            }
            y_tmp += 1;
        }
        let branch_size = self
            .branch_vec
            .iter()
            .map(|x| x.len())
            .collect::<Vec<usize>>();
        self.branch_col_right = self.branch_col_left
            + *branch_size.iter().max().unwrap() as usize
            + self.branch_col_offset;
        self.log_col_left = self.branch_col_right + self.branch_log_gap;
    }
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W) {
        let (x, y) = self.current_pos.unpack();
        let (col, row) = termion::terminal_size().unwrap();
        let x_tmp = self.log_col_left;
        self.log_col_right = col as usize;
        if col <= x_tmp as u16 {
            // No show due to no enough col.
            return;
        }
        self.log_scroll_offset_max =
            if self.right_panel_log_info.len() + self.bar_row_height + self.log_row_top
                <= row as usize
            {
                0
            } else {
                self.right_panel_log_info.len() - row as usize
            };
        let mut y_tmp = self.log_row_top;
        let prev_log_row_bottom = self.log_row_bottom;
        // Log show len (col - x_tmp as u16).
        for log in self.right_panel_log_info[self.log_scroll_offset as usize..].to_vec() {
            // Need to update bottom here.
            self.log_row_bottom = y_tmp;
            self.render_single_line(screen, &log, x_tmp as u16, y_tmp as u16);
            self.row_log_map.insert(y_tmp, log);
            // Spare 2 for check info.
            if y_tmp as u16 >= row - self.bar_row_height as u16 {
                break;
            }
            y_tmp += 1;
        }
        // Clear rest log zone.
        for clear_y in self.log_row_bottom + 1..=prev_log_row_bottom as usize {
            write!(
                screen,
                "{}{}",
                termion::cursor::Goto(x_tmp as u16, clear_y as u16),
                termion::clear::UntilNewline,
            )
            .unwrap();
        }
        write!(screen, "{}", termion::cursor::Goto(x, y)).unwrap();
        screen.flush().unwrap();
    }
    fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
        let (x, y) = self.current_pos.unpack();
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
        let (x, y) = self.current_pos.unpack();
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

    fn reset_cursor_to_current_branch<W: Write>(&mut self, screen: &mut W) {
        // Must update position.
        self.previous_pos = self.current_pos;
        self.current_pos = Position::init(
            1,
            self.get_branch_row(&self.current_branch.to_string())
                .unwrap() as u16,
        );
        self.show_icon_after_cursor(screen, "🏆");
    }
    fn reset_cursor_to_main_branch<W: Write>(&mut self, screen: &mut W) {
        self.current_pos = Position::init(
            1,
            self.get_branch_row(&self.main_branch.to_string()).unwrap() as u16,
        );
        self.previous_pos = self.current_pos;
        self.show_icon_after_cursor_and_wipe(screen, "🌟");
    }
    fn reset_cursor_to_log_top<W: Write>(&mut self, screen: &mut W) {
        // Must update position.
        self.previous_pos = self.current_pos;
        self.current_pos = Position::init(self.log_col_left as u16 - 3, self.log_row_top as u16);
        self.reset_cursor_to_current_pos(screen);
    }

    fn render_single_line<W: Write>(
        &mut self,
        screen: &mut W,
        log: &LogInfoPattern,
        x_tmp: u16,
        y_tmp: u16,
    ) {
        // Clear current line.
        // Refer to https://en.wikipedia.org/wiki/Box-drawing_character#Unicode
        write!(
            screen,
            "{}{}║{}",
            termion::cursor::Goto(x_tmp - 3 as u16, y_tmp as u16),
            termion::clear::UntilNewline,
            termion::cursor::Goto(x_tmp as u16, y_tmp as u16),
        )
        .unwrap();
        let line_width = self.log_col_right - self.log_col_left + 1;
        match log {
            LogInfoPattern::Author(val) | LogInfoPattern::Date(val) | LogInfoPattern::Msg(val) => {
                write!(screen, "{}", val.substring(0, line_width),).unwrap();
            }
            LogInfoPattern::DiffAdd(val) => {
                write!(
                    screen,
                    "{}{}{}",
                    termion::color::Fg(termion::color::Green),
                    val.substring(0, line_width),
                    termion::color::Fg(termion::color::Reset),
                )
                .unwrap();
            }
            LogInfoPattern::DiffSubtract(val) => {
                write!(
                    screen,
                    "{}{}{}",
                    termion::color::Fg(termion::color::Red),
                    val.substring(0, line_width),
                    termion::color::Fg(termion::color::Reset),
                )
                .unwrap();
            }
            LogInfoPattern::Commit(val) => {
                let val = val.substring(0, line_width);
                let split_log: Vec<_> = val.split_inclusive(['(', ',', ')']).collect();
                for tmp in &split_log {
                    if tmp.starts_with("commit") {
                        write!(
                            screen,
                            "{}{}{}",
                            termion::color::Fg(termion::color::LightYellow),
                            tmp,
                            termion::color::Fg(termion::color::Reset),
                        )
                        .unwrap();
                    } else if tmp.starts_with("HEAD ->") {
                        write!(
                            screen,
                            "{}{}HEAD ->{}{}{}{}",
                            termion::color::Fg(termion::color::LightCyan),
                            style::Bold,
                            termion::color::Fg(termion::color::LightGreen),
                            tmp.strip_prefix("HEAD ->").unwrap(),
                            termion::color::Fg(termion::color::Reset),
                            style::Reset,
                        )
                        .unwrap();
                    } else if tmp.contains("origin") {
                        write!(
                            screen,
                            "{}{}{}{}{}",
                            termion::color::Fg(termion::color::LightRed),
                            style::Bold,
                            tmp,
                            termion::color::Fg(termion::color::Reset),
                            style::Reset,
                        )
                        .unwrap();
                    } else {
                        write!(
                            screen,
                            "{}{}{}{}{}",
                            termion::color::Fg(termion::color::LightGreen),
                            style::Bold,
                            tmp,
                            termion::color::Fg(termion::color::Reset),
                            style::Reset,
                        )
                        .unwrap();
                    }
                }
            }
            _ => {}
        }
    }

    fn refresh_frame_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String) {
        // Reset with main branch.
        self.current_branch = branch.to_string();
        self.show_title_in_top_panel(screen);
        self.update_git_branch();
        self.reset_cursor_to_main_branch(screen);

        self.left_panel_handler(screen, MoveDirection::Still);

        self.update_git_log(&self.current_branch.to_string());
        self.right_panel_log_info = self
            .branch_log_info_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.right_panel_handler(screen, MoveDirection::Still);

        screen.flush().unwrap();
    }
    fn reset_cursor_to_current_pos<W: Write>(&mut self, screen: &mut W) {
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
            "{}{}{}",
            termion::cursor::Goto(self.current_pos.col, self.current_pos.row),
            icon,
            termion::cursor::Goto(self.current_pos.col, self.current_pos.row),
        )
        .unwrap();
    }
    fn show_icon_after_cursor_and_wipe<W: Write>(&mut self, screen: &mut W, icon: &str) {
        // Need clear previous position only if with icon drawn.
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

    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection) {
        // Reset something here.
        self.previous_pos = self.current_pos;
        self.log_scroll_offset = 0;

        let (x, mut y) = self.current_pos.unpack();
        match dir {
            MoveDirection::Up => {
                if y > self.branch_row_top as u16 && y <= self.branch_row_bottom as u16 {
                    y = y - 1;
                    if let Ok(ind) = self
                        .branch_vec
                        .binary_search(&self.current_branch.to_string())
                    {
                        self.current_branch = self.branch_vec.to_vec()[ind - 1].to_string();
                    }
                } else {
                    y = self.branch_row_bottom as u16;
                    self.current_branch = self.branch_vec.last().unwrap().to_string();
                }
            }
            MoveDirection::Down => {
                if y >= self.branch_row_top as u16 && y < self.branch_row_bottom as u16 {
                    y = y + 1;
                    if let Ok(ind) = self
                        .branch_vec
                        .binary_search(&self.current_branch.to_string())
                    {
                        self.current_branch = self.branch_vec.to_vec()[ind + 1].to_string();
                    }
                } else {
                    y = self.branch_row_top as u16;
                    self.current_branch = self.branch_vec.first().unwrap().to_string();
                }
            }
            _ => {}
        }
        self.current_pos = Position::init(x, y);

        self.show_branch_in_left_panel(screen);

        // Show the log.
        self.update_git_log(&self.current_branch.to_string());
        self.right_panel_log_info = self
            .branch_log_info_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();

        self.right_panel_handler(screen, MoveDirection::Still);
        screen.flush().unwrap();
    }

    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection) {
        // Reset something here.
        self.previous_pos = self.current_pos;

        let (x, mut y) = self.current_pos.unpack();
        match dir {
            MoveDirection::Down => {
                if y == self.log_row_bottom as u16 {
                    // Hit the bottom.
                    if self.log_scroll_offset < self.log_scroll_offset_max {
                        self.log_scroll_offset += 1;
                        self.show_log_in_right_panel(screen);
                    }
                } else if y < self.log_row_bottom as u16 {
                    y = y + 1;
                }
            }
            MoveDirection::Up => {
                if y == self.log_row_top as u16 {
                    // Hit the top.
                    if self.log_scroll_offset > 0 {
                        self.log_scroll_offset -= 1;
                        self.show_log_in_right_panel(screen);
                    }
                } else {
                    y = y - 1;
                }
            }
            _ => {
                self.show_log_in_right_panel(screen);
            }
        }
        self.current_pos = Position::init(x, y);

        self.show_in_bottom_bar(
            screen,
            &format!(
                "c: {}, r: {}, r_bottom: {}",
                self.current_pos.col, self.current_pos.row, self.log_row_bottom,
            )
            .to_string(),
        );
        screen.flush().unwrap();
    }
}
