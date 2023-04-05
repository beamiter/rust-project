extern crate termion;

pub mod action_git;
pub mod event_git;
pub mod render_git;
pub mod tui_git;

use crate::action_git::*;
use crate::event_git::*;
use crate::render_git::*;
use crate::tui_git::*;

use std::str;
use std::{
    io::{stdin, stdout, Read, Write},
    vec,
};

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;

use coredump::register_panic_handler;

fn main() {
    register_panic_handler().unwrap();
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();

    let mut screen = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();
    // write!(screen, "{}", termion::cursor::Hide).unwrap();

    tui_git.refresh_frame_with_branch(&mut screen, &tui_git.main_branch.to_string());

    // Start with the main branch row.
    for c in stdin().keys() {
        match c.unwrap() {
            Key::Char('b') => {
                // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
                let mut bufs = vec![];
                let mut buffer: &str = "";
                tui_git.show_and_stay_in_status_bar(&mut screen, &"git checkout -b: ".to_string());
                loop {
                    let b = stdin().lock().bytes().next().unwrap().unwrap();
                    match char::from(b) {
                        '\r' | '\n' => {
                            tui_git.checkout_new_git_branch(&mut screen, &buffer.to_string());
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
                    tui_git.show_and_stay_in_status_bar(
                        &mut screen,
                        &format!("git checkout -b: {}", buffer.to_string()).to_string(),
                    );
                }
            }
            Key::Char('c') => {
                // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
                let mut bufs = vec![];
                let mut buffer: &str = "";
                tui_git.show_and_stay_in_status_bar(&mut screen, &"git checkout: ".to_string());
                loop {
                    let b = stdin().lock().bytes().next().unwrap().unwrap();
                    match char::from(b) {
                        '\r' | '\n' => {
                            tui_git.checkout_local_git_branch(&mut screen, &buffer.to_string());
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
                    tui_git.show_and_stay_in_status_bar(
                        &mut screen,
                        &format!("git checkout: {}", buffer.to_string()).to_string(),
                    );
                }
            }
            Key::Char('d') => {
                tui_git.lower_d_pressed(&mut screen);
            }
            Key::Char('f') => {
                // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
                let mut bufs = vec![];
                let mut buffer: &str = "";
                tui_git
                    .show_and_stay_in_status_bar(&mut screen, &"git fetch and check: ".to_string());
                loop {
                    let b = stdin().lock().bytes().next().unwrap().unwrap();
                    match char::from(b) {
                        '\r' | '\n' => {
                            tui_git.checkout_remote_git_branch(&mut screen, &buffer.to_string());
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
                    tui_git.show_and_stay_in_status_bar(
                        &mut screen,
                        &format!("get fetch and check: {}", buffer.to_string()).to_string(),
                    );
                }
            }
            Key::Char('n') | Key::Esc | Key::Char('N') => {
                tui_git.lower_n_pressed(&mut screen);
            }
            Key::Char('q') => {
                break;
            }
            Key::Char('y') | Key::Char('Y') => {
                tui_git.lower_y_pressed(&mut screen);
            }
            Key::Char('D') => {
                tui_git.upper_d_pressed(&mut screen);
            }
            Key::Char('\n') => {
                tui_git.enter_pressed(&mut screen);
            }
            Key::Char(':') => {
                // https://www.ibm.com/docs/en/rdfi/9.6.0?topic=set-escape-sequences
                let mut bufs = vec![];
                let mut buffer = String::new();
                tui_git.show_and_stay_in_status_bar(&mut screen, &"cmd: ".to_string());
                for b in stdin().lock().bytes() {
                    match b {
                        Ok(b'\r') | Ok(b'\n') => {
                            tui_git.execute_normal_command(&mut screen, &buffer.to_string());
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
                    buffer = String::from_utf8(bufs.clone()).unwrap();
                    tui_git.show_and_stay_in_status_bar(
                        &mut screen,
                        &format!("cmd: {}", buffer.to_string()).to_string(),
                    );
                }
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
        screen.flush().unwrap();
    }
    write!(screen, "{}", termion::cursor::Show).unwrap();
    screen.flush().unwrap();
}
