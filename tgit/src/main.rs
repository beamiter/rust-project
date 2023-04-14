pub mod action_git;
pub mod event_git;
pub mod render_git;
pub mod tui_git;

use crate::action_git::*;
use crate::render_git::*;
use crate::tui_git::*;

use coredump::register_panic_handler;

use crossterm::queue;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;

use std::{io::stdout, io::Write, time::Duration};

use futures::{future::FutureExt, select, StreamExt};
use futures_timer::Delay;

use crossterm::{
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
#[allow(dead_code)]
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
                    tui_git.lower_c_pressed(write);
                }
                KeyCode::Char('d') => {
                    tui_git.lower_d_pressed(write);
                }
                KeyCode::Char('f') => {
                    tui_git.lower_f_pressed(write);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    tui_git.lower_n_pressed(write);
                }
                KeyCode::Char('q') => {
                    if tui_git.lower_q_pressed(write) {
                        return true;
                    }
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    tui_git.lower_y_pressed(write);
                }
                KeyCode::Char('D') => {
                    tui_git.upper_d_pressed(write);
                }
                KeyCode::Char(':') => {
                    tui_git.colon_pressed(write);
                }
                KeyCode::Enter => {
                    tui_git.enter_pressed(write);
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('H') => {
                    tui_git.move_cursor_left(write);
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('L') => {
                    tui_git.move_cursor_right(write);
                }
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => {
                    tui_git.move_cursor_up(write);
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => {
                    tui_git.move_cursor_down(write);
                }
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
        Event::Resize(_, _) => {
            // let (original_size, new_size) = flush_resize_events((x, y));
            // println!("Resize from: {:?}, to: {:?}\r", original_size, new_size);
        }
    }
    match tui_git.layout_mode {
        LayoutMode::LeftPanel(DisplayType::Log) => {
            tui_git.async_update = true;
        }
        _ => {
            tui_git.async_update = false;
        }
    }
    write.flush().unwrap();
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
        let mut delay = Delay::new(Duration::from_millis(5_000)).fuse();
        let mut event = reader.next().fuse();

        select! {
            _  = delay => {
                if tui_git.async_update {
                    tui_git.update_git_branch_async();
                    tui_git.refresh_frame_with_branch(write, &tui_git.current_branch.to_string());
                }
            },
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

    enable_raw_mode()?;

    let mut stdout = stdout();
    execute!(stdout, EnableMouseCapture)?;

    run_app(&mut stdout).await?;

    execute!(stdout, DisableMouseCapture)?;

    disable_raw_mode()
}
