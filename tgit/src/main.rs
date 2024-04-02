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

// use async_std::task;

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;

use coredump::register_panic_handler;

fn main() {
    register_panic_handler().unwrap();
    let tui_git_arc = Arc::new(Mutex::new(TuiGit::new()));
    if !tui_git_arc.lock().unwrap().update_git_branch() {
        return;
    }
    let mut screen = Arc::new(Mutex::new(
        stdout()
            .into_raw_mode()
            .unwrap()
            .into_alternate_screen()
            .unwrap(),
    ));

    let mut handles = vec![];
    // Create a thread to update data in the background.
    let hold_confirm = Arc::new(Mutex::new(false));
    let terminated = Arc::new(Mutex::new(false));
    {
        let tui_git_arc = Arc::clone(&tui_git_arc);
        let screen = Arc::clone(&mut screen);

        let hold_confirm = Arc::clone(&hold_confirm);
        let terminated = Arc::clone(&terminated);

        let handle = thread::spawn(move || loop {
            if *terminated.lock().unwrap() {
                break;
            }
            if !*hold_confirm.lock().unwrap() {
                tui_git_arc.lock().unwrap().update_git_branch_async();
                let current_branch = tui_git_arc.lock().unwrap().current_branch.to_string();
                let mut screen = screen.lock().unwrap();
                tui_git_arc
                    .lock()
                    .unwrap()
                    .refresh_frame_with_branch(&mut *screen, &current_branch);
                tui_git_arc
                    .lock()
                    .unwrap()
                    .show_in_status_bar(&mut *screen, &"Update data async.".to_string());
            }
            // async_std::task::sleep::(Duration::from_millis(1_000)).await;
            thread::sleep(Duration::from_secs(4));
        });
        handles.push(handle);
    }

    // write!(screen, "{}", termion::cursor::Hide).unwrap();

    // Start with the main branch row.
    for c in stdin().keys() {
        // Lock the tui_git_arc and update main branch and branch vector.
        let mut tui_git_arc = tui_git_arc.lock().unwrap();
        let mut screen = screen.lock().unwrap();
        let mut hold_confirm = hold_confirm.lock().unwrap();
        *hold_confirm = true;
        match c.unwrap() {
            Key::Char('b') => {
                tui_git_arc.lower_b_pressed(&mut *screen);
            }
            Key::Char('c') => {
                tui_git_arc.lower_c_pressed(&mut *screen);
            }
            Key::Char('d') => {
                tui_git_arc.lower_d_pressed(&mut *screen);
            }
            Key::Char('f') => {
                tui_git_arc.lower_f_pressed(&mut *screen);
            }
            Key::Char('n') | Key::Esc | Key::Char('N') => {
                tui_git_arc.lower_n_pressed(&mut *screen);
            }
            Key::Char('q') | Key::Char('Q') => {
                if tui_git_arc.lower_q_pressed(&mut *screen) {
                    let mut terminated = terminated.lock().unwrap();
                    *terminated = true;
                    break;
                }
            }
            Key::Char('y') | Key::Char('Y') => {
                tui_git_arc.lower_y_pressed(&mut *screen);
            }

            Key::Char('D') => {
                tui_git_arc.upper_d_pressed(&mut *screen);
            }

            Key::Char(':') => {
                tui_git_arc.colon_pressed(&mut *screen);
            }
            Key::Char('\n') => {
                tui_git_arc.enter_pressed(&mut *screen);
            }

            Key::Left | Key::Char('h') | Key::Char('H') => {
                tui_git_arc.move_cursor_left(&mut *screen);
            }
            Key::Right | Key::Char('l') | Key::Char('L') => {
                tui_git_arc.move_cursor_right(&mut *screen);
            }
            Key::Up | Key::Char('k') | Key::Char('K') => {
                tui_git_arc.move_cursor_up(&mut *screen);
            }
            Key::Down | Key::Char('j') | Key::Char('J') => {
                tui_git_arc.move_cursor_down(&mut *screen);
            }
            _ => {}
        }
        // Flush after key pressed.
        screen.flush().unwrap();
        match tui_git_arc.layout_mode {
            LayoutMode::LeftPanel(DisplayType::Log) => {
                *hold_confirm = false;
            }
            _ => {
                *hold_confirm = true;
            }
        }
    }
    // write!(screen, "{}", termion::cursor::Show).unwrap();
    // screen.lock().unwrap().flush().unwrap();

    for handle in handles {
        handle.join().unwrap();
    }
}
