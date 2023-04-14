use crate::tui_git::*;

use std::io::Write;
use std::str;
use substring::Substring;

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::Attribute;
use crossterm::style::Color;
use crossterm::style::Print;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use crossterm::style::Stylize;
use crossterm::terminal::size;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;

pub trait RenderGit {
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W);
    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W);
    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W);
    //
    // fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    // fn show_and_stay_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    // fn show_in_bottom_bar<W: Write>(&mut self, screen: &mut W, log: &String);
    //
    // fn reset_cursor_to_current_pos<W: Write>(&mut self, screen: &mut W);
    // fn reset_cursor_to_log_top<W: Write>(&mut self, screen: &mut W);
    fn reset_cursor_to_branch<W: Write>(&mut self, screen: &mut W, branch: &String);

    fn render_single_line<W: Write>(
        &mut self,
        screen: &mut W,
        log: &LogInfoPattern,
        x_tmp: u16,
        y_tmp: u16,
    );

    fn refresh_frame_with_branch<W: Write>(&mut self, screen: &mut W, branch: &String);
    //
    fn left_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection);
    fn right_panel_handler<W: Write>(&mut self, screen: &mut W, dir: MoveDirection);
    //
    fn show_icon_after_cursor<W: Write>(&mut self, screen: &mut W, icon: &str);
    fn show_icon_after_cursor_and_wipe<W: Write>(&mut self, screen: &mut W, icon: &str);
}

impl RenderGit for TuiGit {
    // https://unix.stackexchange.com/questions/559708/how-to-draw-a-continuous-line-in-terminal
    fn show_title_in_top_panel<W: Write>(&mut self, screen: &mut W) {
        let (col, row) = size().unwrap();
        queue!(
            screen,
            MoveTo(19, 1),
            Clear(ClearType::CurrentLine),
            Print("Welcome to tui git".bold().italic().magenta().on_grey()),
            MoveTo(1, 2),
            Print("⎽".repeat(col as usize - 1)),
            MoveTo(1, row - self.bar_row_height as u16 + 1),
            Print("⎺".repeat(col as usize - 1)),
        )
        .unwrap();
    }
    fn show_branch_in_left_panel<W: Write>(&mut self, screen: &mut W) {
        let (x, y) = self.current_pos.unpack();
        let (col, row) = size().unwrap();
        let x_tmp = self.branch_col_left;
        if col <= x_tmp as u16 {
            // No show due to no enough col.
            return;
        }
        // Clear previous branch zone.
        for clear_y in self.branch_row_top..row as usize - self.bar_row_height {
            queue!(
                screen,
                MoveTo(1, clear_y as u16),
                Clear(ClearType::CurrentLine)
            )
            .unwrap();
        }
        let mut y_tmp = self.branch_row_top;
        for branch in self.branch_vec.to_vec() {
            // Need to update bottom here.
            self.branch_row_bottom = y_tmp;

            if self.branch_delete_set.get(&branch).is_some() {
                queue!(
                    screen,
                    MoveTo(self.branch_col_left as u16, y_tmp as u16),
                    Print(branch.red()),
                )
                .unwrap();
            } else if *branch == self.main_branch && *branch == self.current_branch {
                self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
                queue!(
                    screen,
                    MoveTo(1, y_tmp as u16),
                    Print(UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]),
                    MoveTo(self.branch_col_left as u16, y_tmp as u16),
                    Print(branch.bold().green().underline(Color::White)),
                )
                .unwrap();
            } else if *branch == self.main_branch {
                queue!(
                    screen,
                    MoveTo(self.branch_col_left as u16, y_tmp as u16),
                    Print(branch.italic().green()),
                    Print(" 🐝"),
                )
                .unwrap();
            } else if *branch == self.current_branch {
                self.key_move_counter = (self.key_move_counter + 1) % usize::MAX;
                queue!(
                    screen,
                    MoveTo(1, y_tmp as u16),
                    Print(UNICODE_TABLE[self.key_move_counter % UNICODE_TABLE.len()]),
                    MoveTo(self.branch_col_left as u16, y_tmp as u16),
                    Print(branch.bold().underline(Color::White)),
                )
                .unwrap();
            } else {
                queue!(
                    screen,
                    MoveTo(self.branch_col_left as u16, y_tmp as u16),
                    Print(branch),
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

        queue!(screen, MoveTo(x, y)).unwrap();
        screen.flush().unwrap();
    }

    fn show_log_in_right_panel<W: Write>(&mut self, screen: &mut W) {
        let (x, y) = self.current_pos.unpack();
        let (col, row) = size().unwrap();
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
            queue!(
                screen,
                MoveTo(x_tmp as u16, clear_y as u16),
                Clear(ClearType::CurrentLine),
            )
            .unwrap();
        }

        queue!(screen, MoveTo(x, y)).unwrap();
        screen.flush().unwrap();
    }

    // fn show_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
    //     let (x, y) = self.current_pos.unpack();
    //     let (col, row) = termion::terminal_size().unwrap();
    //     self.status_bar_row = row as usize - 1;
    //     queue!(
    //         screen,
    //         "{}{}{}",
    //         MoveTo(1, self.status_bar_row as u16),
    //         termion::clear::CurrentLine,
    //         color::Fg(color::LightYellow),
    //     )
    //     .unwrap();
    //     queue!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
    //     queue!(
    //         screen,
    //         "{}{}",
    //         color::Fg(color::Reset),
    //         MoveTo(x, y)
    //     )
    //     .unwrap();
    //     screen.flush().unwrap();
    // }
    // fn show_and_stay_in_status_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
    //     let (col, row) = termion::terminal_size().unwrap();
    //     self.status_bar_row = row as usize - 1;
    //     queue!(
    //         screen,
    //         "{}{}{}",
    //         MoveTo(1, self.status_bar_row as u16),
    //         termion::clear::CurrentLine,
    //         color::Fg(color::LightYellow),
    //     )
    //     .unwrap();
    //     queue!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
    //     screen.flush().unwrap();
    // }
    // fn show_in_bottom_bar<W: Write>(&mut self, screen: &mut W, log: &String) {
    //     let (x, y) = self.current_pos.unpack();
    //     let (col, row) = termion::terminal_size().unwrap();
    //     self.bottom_bar_row = row as usize;
    //     queue!(
    //         screen,
    //         "{}{}{}",
    //         MoveTo(1, self.bottom_bar_row as u16),
    //         termion::clear::CurrentLine,
    //         color::Fg(color::Yellow),
    //     )
    //     .unwrap();
    //     queue!(screen, "{}", &log.as_str()[..(col as usize).min(log.len())]).unwrap();
    //     queue!(
    //         screen,
    //         "{}{}",
    //         color::Fg(color::Reset),
    //         MoveTo(x, y)
    //     )
    //     .unwrap();
    //     screen.flush().unwrap();
    // }

    fn reset_cursor_to_branch<W: Write>(&mut self, screen: &mut W, branch: &String) {
        // Must update position.
        self.previous_pos = self.current_pos;
        self.current_pos = Position::init(1, self.get_branch_row(branch).unwrap() as u16);
        self.show_icon_after_cursor(screen, "🏆");
    }

    // fn reset_cursor_to_log_top<W: Write>(&mut self, screen: &mut W) {
    //     // Must update position.
    //     self.previous_pos = self.current_pos;
    //     self.current_pos = Position::init(self.log_col_left as u16 - 3, self.log_row_top as u16);
    //     self.reset_cursor_to_current_pos(screen);
    // }

    fn render_single_line<W: Write>(
        &mut self,
        screen: &mut W,
        log: &LogInfoPattern,
        x_tmp: u16,
        y_tmp: u16,
    ) {
        // Clear current line.
        // Refer to https://en.wikipedia.org/wiki/Box-drawing_character#Unicode
        queue!(
            screen,
            MoveTo(x_tmp - 3 as u16, y_tmp as u16),
            Clear(ClearType::UntilNewLine),
            Print("║"),
            MoveTo(x_tmp as u16, y_tmp as u16),
        )
        .unwrap();
        let line_width = self.log_col_right - self.log_col_left + 1;
        match log {
            LogInfoPattern::Author(val) | LogInfoPattern::Date(val) | LogInfoPattern::Msg(val) => {
                queue!(screen, Print(val.substring(0, line_width))).unwrap();
            }
            LogInfoPattern::DiffAdd(val) => {
                queue!(screen, Print(val.substring(0, line_width).green()),).unwrap();
            }
            LogInfoPattern::DiffSubtract(val) => {
                queue!(screen, Print(val.substring(0, line_width).red()),).unwrap();
            }
            LogInfoPattern::Commit(val) => {
                let val = val.substring(0, line_width);
                let split_log: Vec<_> = val.split_inclusive(['(', ',', ')']).collect();
                for tmp in &split_log {
                    if tmp.starts_with("commit") {
                        queue!(screen, Print(tmp.yellow()),).unwrap();
                    } else if tmp.starts_with("HEAD ->") {
                        queue!(
                            screen,
                            Print(String::from("HEAD ->").cyan().bold()),
                            Print(tmp.strip_prefix("HEAD ->").unwrap().green().bold()),
                        )
                        .unwrap();
                    } else if tmp.contains("origin") {
                        queue!(screen, Print(tmp.red().bold()),).unwrap();
                    } else {
                        queue!(screen, Print(tmp.green().bold()),).unwrap();
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

        self.left_panel_handler(screen, MoveDirection::Still);

        self.update_git_log(&self.current_branch.to_string());
        self.right_panel_log_info = self
            .branch_log_info_map
            .get(&self.current_branch.to_string())
            .unwrap()
            .to_vec();
        self.right_panel_handler(screen, MoveDirection::Still);

        self.reset_cursor_to_branch(screen, branch);
        screen.flush().unwrap();
    }
    // fn reset_cursor_to_current_pos<W: Write>(&mut self, screen: &mut W) {
    //     queue!(
    //         screen,
    //         "{}",
    //         MoveTo(self.current_pos.col, self.current_pos.row),
    //     )
    //     .unwrap();
    // }

    fn show_icon_after_cursor<W: Write>(&mut self, screen: &mut W, icon: &str) {
        queue!(
            screen,
            MoveTo(self.current_pos.col, self.current_pos.row),
            Print(icon),
            MoveTo(self.current_pos.col, self.current_pos.row),
        )
        .unwrap();
    }

    fn show_icon_after_cursor_and_wipe<W: Write>(&mut self, screen: &mut W, icon: &str) {
        // Need clear previous position only if with icon drawn.
        queue!(
            screen,
            MoveTo(self.previous_pos.col, self.previous_pos.row),
            Print(" ".repeat(2)),
        )
        .unwrap();
        queue!(
            screen,
            MoveTo(self.current_pos.col, self.current_pos.row),
            Print(icon),
            MoveTo(self.current_pos.col, self.current_pos.row),
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

        // self.show_in_bottom_bar(
        //     screen,
        //     &format!(
        //         "c: {}, r: {}, r_bottom: {}",
        //         self.current_pos.col, self.current_pos.row, self.log_row_bottom,
        //     )
        //     .to_string(),
        // );
        screen.flush().unwrap();
    }
}
