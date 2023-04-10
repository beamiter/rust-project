extern crate termion;
use crate::render_git::*;
use crate::tui_git::*;

use std::{io::Write, process::Command, str};
use termion::color;

pub trait EventGit {
    fn checkout_local_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn checkout_new_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn checkout_remote_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn delete_git_branch<W: Write>(&mut self, screen: &mut W) -> bool;
    fn execute_normal_command<W: Write>(&mut self, screen: &mut W, command: &str) -> bool;
}

impl EventGit for TuiGit {
    fn delete_git_branch<W: Write>(&mut self, screen: &mut W) -> bool {
        for branch in self.branch_delete_set.to_owned() {
            let output = Command::new("git")
                .args(["branch", "-D", branch.as_str()])
                .output()
                .expect("failed to execute process");
            if !output.status.success() {
                self.show_in_status_bar(
                    screen,
                    &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr)).to_string(),
                );
                return false;
            } else {
                self.show_in_status_bar(
                    screen,
                    &format!(
                        "✅ Delete branch {}{}{} finished.",
                        color::Fg(color::Red),
                        branch,
                        color::Fg(color::LightYellow),
                    )
                    .to_string(),
                );
            }
        }
        self.branch_delete_set.clear();
        self.refresh_frame_with_branch(screen, &self.main_branch.to_string());
        return true;
    }
    fn checkout_remote_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool {
        let output = Command::new("git")
            .args(["fetch", "origin", branch.as_str()])
            .output()
            .expect("failed to execute process");
        if !output.status.success() {
            self.show_in_status_bar(
                screen,
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr)).to_string(),
            );
            return false;
        } else {
            self.show_in_status_bar(
                screen,
                &format!(
                    "✅ Fetch origin branch {}{}{} succeed.",
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::LightYellow),
                )
                .to_string(),
            );
        }
        return self.checkout_local_git_branch(screen, branch);
    }
    fn checkout_local_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool {
        if branch == &self.main_branch {
            self.show_in_status_bar(
                screen,
                &format!(
                    "👻 Already in target branch {}{}{}, enter 'q' to quit.",
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::LightYellow),
                )
                .to_string(),
            );
            return true;
        }
        let output = Command::new("git")
            .args(["checkout", branch.as_str()])
            .output()
            .expect("failed to execute process");
        if !output.status.success() {
            self.show_in_status_bar(
                screen,
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr)).to_string(),
            );
        } else {
            self.main_branch = branch.to_string();
            self.show_in_status_bar(
                screen,
                &format!(
                    "✅ Checkout to target branch {}{}{}, enter 'q' to quit",
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::LightYellow),
                )
                .to_string(),
            );
            self.refresh_frame_with_branch(screen, &self.main_branch.to_string());
        }
        output.status.success()
    }
    fn checkout_new_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool {
        if branch == &self.main_branch {
            self.show_in_status_bar(
                screen,
                &format!(
                    "👻 Already in target branch {}{}{}, enter 'q' to quit.",
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::LightYellow),
                )
                .to_string(),
            );
            return true;
        }
        let output = Command::new("git")
            .args(["checkout", "-b", branch.as_str()])
            .output()
            .expect("failed to execute process");
        if !output.status.success() {
            self.show_in_status_bar(
                screen,
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr)).to_string(),
            );
        } else {
            self.main_branch = branch.to_string();
            self.show_in_status_bar(
                screen,
                &format!(
                    "✅ Checkout to target branch {}{}{}, enter 'q' to quit",
                    color::Fg(color::Green),
                    branch,
                    color::Fg(color::LightYellow),
                )
                .to_string(),
            );
            self.refresh_frame_with_branch(screen, &self.main_branch.to_string());
        }
        output.status.success()
    }
    fn execute_normal_command<W: Write>(&mut self, screen: &mut W, command: &str) -> bool {
        let mut arrow_escape: Vec<char> = vec![];
        let mut bufs: Vec<u8> = vec![];
        bufs.resize(command.len(), b' ');
        let mut i: usize = 0;
        for c in command.bytes() {
            match c {
                0x1b => {
                    arrow_escape.push(char::from(c));
                }
                b'[' => {
                    if let Some('\u{1b}') = arrow_escape.last() {
                        arrow_escape.push(char::from(c));
                    }
                }
                _ => {
                    if let Some('[') = arrow_escape.last() {
                        arrow_escape.push(char::from(c));
                        continue;
                    }
                    bufs[i] = c;
                }
            }
            i += 1;
        }
        // Truncate the pre-reserved.
        bufs.truncate(bufs.len() - arrow_escape.len());
        let mut bufs_vec = bufs.split(|&x| x == b'"');
        let mut buffers: Vec<String> = vec![];
        loop {
            if let Some(buf) = bufs_vec.next() {
                if !buf.is_empty() {
                    buffers.push(String::from_utf8(buf.to_vec()).unwrap());
                }
            } else {
                break;
            }
        }
        if buffers.is_empty() {
            self.show_in_status_bar(screen, &format!("🔘 buffer is empty").to_string());
            return false;
        }
        let mut buffers_iter = buffers.into_iter();
        let mut command_vec: Vec<String> = vec![];
        if let Some(buffer) = buffers_iter.next() {
            command_vec.append(
                &mut buffer
                    .split_whitespace()
                    .map(|x| String::from(x))
                    .collect::<Vec<String>>(),
            );
        }
        if let Some(buffer) = buffers_iter.next() {
            command_vec.push(buffer);
        }
        // self.show_in_status_bar(
        //     screen,
        //     &format!("🟢 total command seq: {:?}", command_vec).to_string(),
        // );
        let output = match command_vec.len() {
            1 => Command::new(&command_vec[0]).output(),
            2 => Command::new(&command_vec[0]).arg(&command_vec[1]).output(),
            _ => Command::new(&command_vec[0])
                .args(&command_vec[1..])
                .output(),
        };
        match output {
            Ok(output) => {
                if !output.status.success() {
                    self.show_in_status_bar(
                        screen,
                        &format!(
                            "🔘 {:?}",
                            if output.stdout.is_empty() {
                                format!("{:?} error", command_vec).to_string()
                            } else {
                                String::from_utf8_lossy(&output.stderr).to_string()
                            }
                        )
                        .to_string(),
                    );
                    false
                } else {
                    self.show_in_status_bar(
                        screen,
                        &format!(
                            "🟢 {:?}",
                            if output.stdout.is_empty() {
                                format!("{:?} succeed", command_vec).to_string()
                            } else {
                                String::from_utf8_lossy(&output.stdout).to_string()
                            }
                        )
                        .to_string(),
                    );
                    true
                }
            }
            Err(output) => {
                self.show_in_status_bar(
                    screen,
                    &format!("🔘 {:?}", output.to_string(),).to_string(),
                );
                false
            }
        }
    }
}
