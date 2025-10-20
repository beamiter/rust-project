// src/backend/x11/event_source.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto as x;
use x11rb::protocol::Event as XEvent;

use crate::backend::api::{BackendEvent, EventSource, WindowId};

pub struct X11EventSource<C: Connection> {
    conn: Arc<C>,
}

impl<C: Connection> X11EventSource<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self { conn }
    }

    fn map_event(ev: XEvent) -> Option<BackendEvent> {
        match ev {
            XEvent::ButtonPress(e) => Some(BackendEvent::ButtonPress {
                window: WindowId(e.event as u64),
                state: e.state.bits(),
                detail: e.detail,
                time: e.time,
            }),
            XEvent::ButtonRelease(e) => Some(BackendEvent::ButtonRelease {
                window: WindowId(e.event as u64),
                time: e.time,
            }),
            XEvent::MotionNotify(e) => Some(BackendEvent::MotionNotify {
                window: WindowId(e.event as u64),
                root_x: e.root_x,
                root_y: e.root_y,
                time: e.time,
            }),
            XEvent::KeyPress(e) => Some(BackendEvent::KeyPress {
                keycode: e.detail,
                state: e.state.bits(),
            }),
            XEvent::MappingNotify(e) => Some(BackendEvent::MappingNotify {
                request: u8::from(e.request),
            }),
            XEvent::ClientMessage(e) => {
                let d = e.data.as_data32();
                Some(BackendEvent::ClientMessage {
                    window: WindowId(e.window as u64),
                    type_: e.type_,
                    data: [d[0], d[1], d[2], d[3], d[4]],
                    format: e.format,
                })
            }
            XEvent::ConfigureRequest(e) => Some(BackendEvent::ConfigureRequest {
                window: WindowId(e.window as u64),
                mask: e.value_mask.bits(),
                x: e.x,
                y: e.y,
                w: e.width,
                h: e.height,
                border: e.border_width,
                sibling: if e.value_mask.contains(x::ConfigWindow::SIBLING) {
                    Some(WindowId(e.sibling as u64))
                } else {
                    None
                },
                stack_mode: u8::try_from(u32::from(e.stack_mode)).unwrap(),
            }),
            XEvent::ConfigureNotify(e) => Some(BackendEvent::ConfigureNotify {
                window: WindowId(e.window as u64),
                x: e.x,
                y: e.y,
                w: e.width,
                h: e.height,
            }),
            XEvent::DestroyNotify(e) => Some(BackendEvent::DestroyNotify {
                window: WindowId(e.window as u64),
            }),
            XEvent::EnterNotify(e) => Some(BackendEvent::EnterNotify {
                window: WindowId(e.root as u64),
                event: WindowId(e.event as u64),
                mode: e.mode.into(),
                detail: e.detail.into(),
            }),
            XEvent::Expose(e) => Some(BackendEvent::Expose {
                window: WindowId(e.window as u64),
                count: e.count,
            }),
            XEvent::FocusIn(e) => Some(BackendEvent::FocusIn {
                event: WindowId(e.event as u64),
            }),
            XEvent::MapRequest(e) => Some(BackendEvent::MapRequest {
                window: WindowId(e.window as u64),
            }),
            XEvent::PropertyNotify(e) => Some(BackendEvent::PropertyNotify {
                window: WindowId(e.window as u64),
                atom: e.atom,
                state: e.state.into(),
            }),
            XEvent::UnmapNotify(e) => Some(BackendEvent::UnmapNotify {
                window: WindowId(e.window as u64),
                from_configure: e.from_configure,
            }),
            _ => None,
        }
    }
}

impl<C: Connection + Send + Sync + 'static> EventSource for X11EventSource<C> {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        let ev = self.conn.poll_for_event()?;
        Ok(ev.and_then(Self::map_event))
    }

    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }
}
