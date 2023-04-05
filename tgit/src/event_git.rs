extern crate termion;
use crate::render_git::*;
use crate::tui_git::*;

use std::{io::Write, process::Command};
use termion::color;

pub trait EventGit {
    fn checkout_local_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn checkout_new_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn checkout_remote_git_branch<W: Write>(&mut self, screen: &mut W, branch: &String) -> bool;
    fn delete_git_branch<W: Write>(&mut self, screen: &mut W) -> bool;
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
                    &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr),).to_string(),
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
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr),).to_string(),
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
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr),).to_string(),
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
                &format!("❌ {:?}", String::from_utf8_lossy(&output.stderr),).to_string(),
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
}
