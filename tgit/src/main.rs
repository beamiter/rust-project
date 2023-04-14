pub mod action_git;
pub mod event_git;
pub mod render_git;
pub mod tui_git;

use crate::action_git::*;
use crate::event_git::*;
use crate::render_git::*;
use crate::tui_git::*;

use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use coredump::register_panic_handler;
use crossterm::queue;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;

use std::{io::stdout, io::Write, time::Duration};

use futures::{future::FutureExt, select, StreamExt};
use futures_timer::Delay;

use crossterm::{
    cursor::position,
    event::{
        poll, read, DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent,
        KeyModifiers, MouseEvent,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

// Resize events can occur in batches.
// With a simple loop they can be flushed.
// This function will keep the first and last resize event.
fn flush_resize_events(first_resize: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    let mut last_resize = first_resize;
    while let Ok(true) = poll(Duration::from_millis(50)) {
        if let Ok(Event::Resize(x, y)) = read() {
            last_resize = (x, y);
        }
    }
    (first_resize, last_resize)
}
fn match_event_and_break<W: Write>(tui_git: &mut TuiGit, write: &mut W, event: Event) -> bool {
    // println!("Event:: {:?}\r", event);
    // if event == Event::Key(KeyCode::Char('c').into()) {}
    // if let Event::Resize(x, y) = event {}
    // if event == Event::Key(KeyCode::Esc.into()) {}
    match event {
        Event::Key(key) => match key {
            KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                ..
            } => match code {
                KeyCode::Char('b') => {
                    tui_git.lower_b_pressed(write);
                }
                KeyCode::Char('c') => {
                    // println!("Cursor position: {:?}\r", position());
                }
                KeyCode::Char('d') => {}
                KeyCode::Char('f') => {}
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {}
                KeyCode::Char('q') => {
                    return true;
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {}
                KeyCode::Char('D') => {}
                KeyCode::Char(':') => {}
                KeyCode::Enter => {}
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => {}
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => {}
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {}
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {}
                _ => {}
            },
            _ => {}
        },
        Event::FocusLost => {}
        Event::FocusGained => {}
        Event::Mouse(mouse) => match mouse {
            MouseEvent { .. } => {}
        },
        Event::Paste(_) => {}
        Event::Resize(x, y) => {
            let (original_size, new_size) = flush_resize_events((x, y));
            // println!("Resize from: {:?}, to: {:?}\r", original_size, new_size);
        }
    }
    return false;
}

async fn run_app<W>(write: &mut W) -> std::io::Result<()>
where
    W: Write,
{
    let mut tui_git = TuiGit::new();
    tui_git.update_git_branch();
    // execute or queue.
    queue!(write, EnterAlternateScreen)?;
    write.flush()?;
    tui_git.refresh_frame_with_branch(write, &tui_git.main_branch.to_string());
    let mut reader = EventStream::new();
    loop {
        let mut delay = Delay::new(Duration::from_millis(1_000)).fuse();
        let mut event = reader.next().fuse();

        select! {
            _  = delay => {},
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if match_event_and_break(&mut tui_git, write, event) {
                            break;
                        }
                    }
                    Some(Err(e)) => println!("Error: {:?}\r", e),
                    None => break,
                }
            }
        };
    }

    execute!(write, LeaveAlternateScreen)?;

    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    register_panic_handler().unwrap();

    // Create a thread to update data in the background.
    // let update_confirm = Arc::new(Mutex::new(false));
    // let hold_confirm = Arc::new(Mutex::new(false));
    // let tui_git_arc = Arc::new(Mutex::new(TuiGit::new()));
    // {
    //     let tui_git_arc = Arc::clone(&tui_git_arc);
    //     let update_confirm = Arc::clone(&update_confirm);
    //     let hold_confirm = Arc::clone(&hold_confirm);
    //     let _ = thread::spawn(move || loop {
    //         if !*hold_confirm.lock().unwrap() {
    //             tui_git_arc.lock().unwrap().update_git_branch_async();
    //             *update_confirm.lock().unwrap() = true;
    //         } else {
    //             *update_confirm.lock().unwrap() = false;
    //         }
    //         thread::sleep(Duration::from_secs(5));
    //     });
    // }
    enable_raw_mode()?;

    let mut stdout = stdout();
    execute!(stdout, EnableMouseCapture)?;

    run_app(&mut stdout).await?;

    execute!(stdout, DisableMouseCapture)?;

    // for c in stdin().keys() {
    //     // Lock the tui_git_arc and update main branch and branch vector.
    //     let mut update_confirm = update_confirm.lock().unwrap();
    //     if *update_confirm {
    //         *update_confirm = false;
    //
    //         let tui_git_arc = tui_git_arc.lock().unwrap();
    //         tui_git.main_branch = tui_git_arc.main_branch.to_string();
    //         tui_git.branch_vec = tui_git_arc.branch_vec.to_vec();
    //         tui_git.branch_log_info_map = tui_git_arc.branch_log_info_map.clone();
    //
    //         tui_git.refresh_frame_with_branch(&mut screen, &tui_git.current_branch.to_string());
    //         tui_git.show_in_status_bar(&mut screen, &"Update data async.".to_string());
    //     }
    //     let mut hold_confirm = hold_confirm.lock().unwrap();
    //     *hold_confirm = true;
    //     match c.unwrap() {
    //         Key::Char('b') => {
    //             tui_git.lower_b_pressed(&mut screen);
    //         }
    //         Key::Char('c') => {
    //             tui_git.lower_c_pressed(&mut screen);
    //         }
    //         Key::Char('d') => {
    //             tui_git.lower_d_pressed(&mut screen);
    //         }
    //         Key::Char('f') => {
    //             tui_git.lower_f_pressed(&mut screen);
    //         }
    //         Key::Char('n') | Key::Esc | Key::Char('N') => {
    //             tui_git.lower_n_pressed(&mut screen);
    //         }
    //         Key::Char('q') => {
    //             if tui_git.lower_q_pressed(&mut screen) {
    //                 break;
    //             }
    //         }
    //         Key::Char('y') | Key::Char('Y') => {
    //             tui_git.lower_y_pressed(&mut screen);
    //         }
    //
    //         Key::Char('D') => {
    //             tui_git.upper_d_pressed(&mut screen);
    //         }
    //
    //         Key::Char(':') => {
    //             tui_git.colon_pressed(&mut screen);
    //         }
    //         Key::Char('\n') => {
    //             tui_git.enter_pressed(&mut screen);
    //         }
    //
    //         Key::Left | Key::Char('h') | Key::Char('H') => {
    //             tui_git.move_cursor_left(&mut screen);
    //         }
    //         Key::Right | Key::Char('l') | Key::Char('L') => {
    //             tui_git.move_cursor_right(&mut screen);
    //         }
    //         Key::Up | Key::Char('k') | Key::Char('K') => {
    //             tui_git.move_cursor_up(&mut screen);
    //         }
    //         Key::Down | Key::Char('j') | Key::Char('J') => {
    //             tui_git.move_cursor_down(&mut screen);
    //         }
    //         _ => {}
    //     }
    //     // Flush after key pressed.
    //     screen.flush().unwrap();
    //     match tui_git.layout_mode {
    //         LayoutMode::LeftPanel(DisplayType::Log) => {
    //             *hold_confirm = false;
    //         }
    //         _ => {
    //             *hold_confirm = true;
    //         }
    //     }
    // }
    // // write!(screen, "{}", termion::cursor::Show).unwrap();
    // screen.flush().unwrap();
    disable_raw_mode()
}
