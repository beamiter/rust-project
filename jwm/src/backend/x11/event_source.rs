// src/backend/x11/event_source.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto as x;
use x11rb::protocol::Event as XEvent;

use crate::backend::api::{
    BackendEvent, EventSource, NetWmAction, NetWmState, PropertyKind, WindowId,
};
use crate::backend::x11::Atoms;

pub struct X11EventSource<C: Connection> {
    conn: Arc<C>,
    atoms: Atoms,
    root: u32,
}

impl<C: Connection> X11EventSource<C> {
    pub fn new(conn: Arc<C>, atoms: Atoms, root: u32) -> Self {
        Self { conn, atoms, root }
    }

    fn map_property_kind(&self, atom: u32) -> PropertyKind {
        if atom == self.atoms.WM_TRANSIENT_FOR { PropertyKind::WmTransientFor }
        else if atom == u32::from(x::AtomEnum::WM_NORMAL_HINTS) { PropertyKind::WmNormalHints }
        else if atom == u32::from(x::AtomEnum::WM_HINTS) { PropertyKind::WmHints }
        else if atom == u32::from(x::AtomEnum::WM_NAME) { PropertyKind::WmName }
        else if atom == self.atoms._NET_WM_NAME { PropertyKind::NetWmName }
        else if atom == self.atoms._NET_WM_WINDOW_TYPE { PropertyKind::NetWmWindowType }
        else { PropertyKind::Other }
    }

    fn map_net_wm_action(action: u32) -> Option<NetWmAction> {
        // EWMH: 0=Remove,1=Add,2=Toggle
        match action {
            0 => Some(NetWmAction::Remove),
            1 => Some(NetWmAction::Add),
            2 => Some(NetWmAction::Toggle),
            _ => None,
        }
    }

    fn map_event(&self, ev: XEvent) -> Option<BackendEvent> {
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
                // 只在我们关心的 EWMH 类型时转成语义事件，否则原样透传
                let data32 = e.data.as_data32();
                if e.type_ == self.atoms._NET_WM_STATE && e.format == 32 && data32.len() >= 3 {
                    let window = WindowId(e.window as u64);
                    if let Some(action) = Self::map_net_wm_action(data32[0]) {
                        let s1 = if data32[1] == self.atoms._NET_WM_STATE_FULLSCREEN {
                            Some(NetWmState::Fullscreen)
                        } else {
                            None
                        };
                        let s2 = if data32[2] == self.atoms._NET_WM_STATE_FULLSCREEN {
                            Some(NetWmState::Fullscreen)
                        } else {
                            None
                        };
                        return Some(BackendEvent::EwmhState {
                            window,
                            action,
                            states: [s1, s2],
                        });
                    }
                }
                if e.type_ == self.atoms._NET_ACTIVE_WINDOW {
                    return Some(BackendEvent::ActiveWindowMessage {
                        window: WindowId(e.window as u64),
                    });
                }
                // 其它类型按原样透传（若上层需要）
                Some(BackendEvent::ClientMessage {
                    window: WindowId(e.window as u64),
                    type_: e.type_,
                    data: [data32[0], data32[1], data32[2], data32[3], data32[4]],
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
                } else { None },
                stack_mode: u8::try_from(u32::from(e.stack_mode)).unwrap(),
            }),
            XEvent::ConfigureNotify(e) => Some(BackendEvent::ConfigureNotify {
                window: WindowId(e.window as u64),
                x: e.x, y: e.y, w: e.width, h: e.height,
            }),
            XEvent::DestroyNotify(e) => Some(BackendEvent::DestroyNotify {
                window: WindowId(e.window as u64),
            }),
            XEvent::EnterNotify(e) => Some(BackendEvent::EnterNotify {
                window: WindowId(self.root as u64), // 维持现有 enter_notify(root, event, ..) 语义
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
            XEvent::PropertyNotify(e) => {
                let kind = self.map_property_kind(e.atom);
                let deleted = e.state == x::Property::DELETE.into();
                Some(BackendEvent::PropertyChanged {
                    window: WindowId(e.window as u64),
                    kind,
                    deleted,
                })
            }
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
        Ok(ev.and_then(|e| self.map_event(e)))
    }
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }
}
