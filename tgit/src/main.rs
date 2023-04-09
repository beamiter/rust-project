extern crate termion;

pub mod action_git;
pub mod event_git;
pub mod render_git;
pub mod tui_git;

use crate::action_git::*;
use crate::render_git::*;
use crate::tui_git::*;

use std::io::{stdin, stdout, Write};

use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;

use coredump::register_panic_handler;

fn main() {
    register_panic_handler().unwrap();
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();

    // Create a thread to update data in the background.
    let tui_git_arc = Arc::new(Mutex::new(TuiGit::new()));
    {
        let tui_git_arc = Arc::clone(&tui_git_arc);
        let _ = thread::spawn(move || loop {
            tui_git_arc.lock().unwrap().update_git_branch();
            thread::sleep(Duration::from_secs(1));
        });
    }

    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    // write!(screen, "{}", termion::cursor::Hide).unwrap();

    tui_git.refresh_frame_with_branch(&mut screen, &tui_git.main_branch.to_string());

    // Start with the main branch row.
    for c in stdin().keys() {
        tui_git.branch_vec = (*tui_git_arc.lock().unwrap()).branch_vec.to_vec();
        tui_git.show_in_status_bar(
            &mut screen,
            &format!("{:?}", tui_git.branch_vec).to_string(),
        );
        tui_git.show_branch_in_left_panel(&mut screen);
        match c.unwrap() {
            Key::Char('b') => {
                tui_git.lower_b_pressed(&mut screen);
            }
            Key::Char('c') => {
                tui_git.lower_c_pressed(&mut screen);
            }
            Key::Char('d') => {
                tui_git.lower_d_pressed(&mut screen);
            }
            Key::Char('f') => {
                tui_git.lower_f_pressed(&mut screen);
            }
            Key::Char('n') | Key::Esc | Key::Char('N') => {
                tui_git.lower_n_pressed(&mut screen);
            }
            Key::Char('q') => {
                if tui_git.lower_q_pressed(&mut screen) {
                    break;
                }
            }
            Key::Char('y') | Key::Char('Y') => {
                tui_git.lower_y_pressed(&mut screen);
            }

            Key::Char('D') => {
                tui_git.upper_d_pressed(&mut screen);
            }

            Key::Char(':') => {
                tui_git.colon_pressed(&mut screen);
            }
            Key::Char('\n') => {
                tui_git.enter_pressed(&mut screen);
            }

            Key::Left | Key::Char('h') => {
                tui_git.move_cursor_left(&mut screen);
            }
            Key::Right | Key::Char('l') => {
                tui_git.move_cursor_right(&mut screen);
            }
            Key::Up | Key::Char('k') => {
                tui_git.move_cursor_up(&mut screen);
            }
            Key::Down | Key::Char('j') => {
                tui_git.move_cursor_down(&mut screen);
            }
            _ => {}
        }
        // Flush after key pressed.
        screen.flush().unwrap();
    }
    // write!(screen, "{}", termion::cursor::Show).unwrap();
    screen.flush().unwrap();
}
