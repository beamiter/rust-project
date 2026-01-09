// src/backend/x11/event_source.rs
use std::os::unix::io::{AsRawFd, BorrowedFd};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::{Event as XEvent, xproto};
use x11rb::rust_connection::RustConnection;

use crate::backend::api::NotifyMode;
use crate::backend::api::{
    BackendEvent, NetWmAction, NetWmState, PropertyKind, StackMode, WindowChanges,
};
use crate::backend::common_define::WindowId;
use crate::backend::x11::Atoms;

use calloop::{EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory};

pub struct X11EventSource {
    conn: Arc<RustConnection>,
    atoms: Atoms,
}

impl X11EventSource {
    pub fn new(conn: Arc<RustConnection>, atoms: Atoms) -> Self {
        Self { conn, atoms }
    }

    fn map_property_kind(&self, atom: u32) -> PropertyKind {
        if atom == self.atoms.WM_TRANSIENT_FOR {
            PropertyKind::TransientFor
        } else if atom == u32::from(xproto::AtomEnum::WM_NORMAL_HINTS) {
            PropertyKind::SizeHints
        } else if atom == u32::from(xproto::AtomEnum::WM_HINTS) {
            PropertyKind::Urgency
        } else if atom == u32::from(xproto::AtomEnum::WM_NAME) || atom == self.atoms._NET_WM_NAME {
            PropertyKind::Title
        } else if atom == u32::from(xproto::AtomEnum::WM_CLASS) {
            PropertyKind::Class
        } else if atom == self.atoms._NET_WM_WINDOW_TYPE {
            PropertyKind::WindowType
        } else if atom == self.atoms.WM_PROTOCOLS {
            PropertyKind::Protocols
        } else {
            PropertyKind::Other
        }
    }

    fn map_net_wm_action(action: u32) -> Option<NetWmAction> {
        match action {
            0 => Some(NetWmAction::Remove),
            1 => Some(NetWmAction::Add),
            2 => Some(NetWmAction::Toggle),
            _ => None,
        }
    }

    fn map_event(&self, ev: XEvent) -> Option<BackendEvent> {
        match ev {
            XEvent::ButtonRelease(e) => Some(BackendEvent::ButtonRelease {
                window: Some(WindowId::X11(e.event as u64)),
                time: e.time,
            }),
            XEvent::ButtonPress(e) => Some(BackendEvent::ButtonPress {
                window: Some(WindowId::X11(e.event as u64)),
                state: e.state.bits(),
                detail: e.detail,
                time: e.time,
                root_x: e.root_x as f64,
                root_y: e.root_y as f64,
            }),
            XEvent::RandrScreenChangeNotify(_) => Some(BackendEvent::ScreenLayoutChanged),
            XEvent::RandrNotify(_) => Some(BackendEvent::ScreenLayoutChanged),
            XEvent::MotionNotify(e) => Some(BackendEvent::MotionNotify {
                window: Some(WindowId::X11(e.event as u64)),
                root_x: e.root_x as f64,
                root_y: e.root_y as f64,
                time: e.time,
            }),
            XEvent::KeyPress(e) => Some(BackendEvent::KeyPress {
                keycode: e.detail,
                state: e.state.bits(),
                time: e.time,
            }),
            XEvent::MapRequest(e) => {
                Some(BackendEvent::WindowCreated(WindowId::X11(e.window as u64)))
            }
            XEvent::MapNotify(e) => {
                Some(BackendEvent::WindowMapped(WindowId::X11(e.window as u64)))
            }
            XEvent::UnmapNotify(e) => {
                Some(BackendEvent::WindowUnmapped(WindowId::X11(e.window as u64)))
            }
            XEvent::DestroyNotify(e) => Some(BackendEvent::WindowDestroyed(WindowId::X11(
                e.window as u64,
            ))),
            XEvent::ConfigureNotify(e) => Some(BackendEvent::WindowConfigured {
                window: WindowId::X11(e.window as u64),
                x: e.x as i32,
                y: e.y as i32,
                width: e.width as u32,
                height: e.height as u32,
            }),
            XEvent::EnterNotify(e) => Some(BackendEvent::EnterNotify {
                window: WindowId::X11(e.event as u64),
                subwindow: if e.child != 0 {
                    Some(WindowId::X11(e.child as u64))
                } else {
                    None
                },
                mode: NotifyMode::Normal,
            }),
            XEvent::LeaveNotify(e) => Some(BackendEvent::LeaveNotify {
                window: WindowId::X11(e.event as u64),
                mode: NotifyMode::Normal,
            }),
            XEvent::FocusIn(e) => Some(BackendEvent::FocusIn {
                window: WindowId::X11(e.event as u64),
            }),
            XEvent::FocusOut(e) => Some(BackendEvent::FocusOut {
                window: WindowId::X11(e.event as u64),
            }),
            XEvent::ConfigureRequest(e) => {
                let changes = WindowChanges {
                    x: if e.value_mask.contains(xproto::ConfigWindow::X) {
                        Some(e.x as i32)
                    } else {
                        None
                    },
                    y: if e.value_mask.contains(xproto::ConfigWindow::Y) {
                        Some(e.y as i32)
                    } else {
                        None
                    },
                    width: if e.value_mask.contains(xproto::ConfigWindow::WIDTH) {
                        Some(e.width as u32)
                    } else {
                        None
                    },
                    height: if e.value_mask.contains(xproto::ConfigWindow::HEIGHT) {
                        Some(e.height as u32)
                    } else {
                        None
                    },
                    border_width: if e.value_mask.contains(xproto::ConfigWindow::BORDER_WIDTH) {
                        Some(e.border_width as u32)
                    } else {
                        None
                    },
                    sibling: if e.value_mask.contains(xproto::ConfigWindow::SIBLING) {
                        Some(WindowId::X11(e.sibling as u64))
                    } else {
                        None
                    },
                    stack_mode: if e.value_mask.contains(xproto::ConfigWindow::STACK_MODE) {
                        match e.stack_mode {
                            xproto::StackMode::ABOVE => Some(StackMode::Above),
                            xproto::StackMode::BELOW => Some(StackMode::Below),
                            xproto::StackMode::TOP_IF => Some(StackMode::TopIf),
                            xproto::StackMode::BOTTOM_IF => Some(StackMode::BottomIf),
                            xproto::StackMode::OPPOSITE => Some(StackMode::Opposite),
                            _ => None,
                        }
                    } else {
                        None
                    },
                };
                Some(BackendEvent::ConfigureRequest {
                    window: WindowId::X11(e.window as u64),
                    mask_bits: e.value_mask.bits(),
                    changes,
                })
            }
            XEvent::PropertyNotify(e) => {
                if e.state == xproto::Property::DELETE.into() {
                    return None;
                }
                let kind = self.map_property_kind(e.atom);
                Some(BackendEvent::PropertyChanged {
                    window: WindowId::X11(e.window as u64),
                    kind,
                })
            }
            XEvent::ClientMessage(e) => {
                let data32 = e.data.as_data32();
                if e.type_ == self.atoms._NET_WM_STATE && e.format == 32 && data32.len() >= 2 {
                    let window = WindowId::X11(e.window as u64);
                    if let Some(action) = Self::map_net_wm_action(data32[0]) {
                        for &atom in &[data32[1], data32[2]] {
                            if atom == self.atoms._NET_WM_STATE_FULLSCREEN {
                                return Some(BackendEvent::WindowStateRequest {
                                    window,
                                    action,
                                    state: NetWmState::Fullscreen,
                                });
                            }
                        }
                    }
                }
                if e.type_ == self.atoms._NET_ACTIVE_WINDOW {
                    return Some(BackendEvent::ActiveWindowMessage {
                        window: WindowId::X11(e.window as u64),
                    });
                }
                Some(BackendEvent::ClientMessage {
                    window: WindowId::X11(e.window as u64),
                    type_: e.type_,
                    data: [
                        data32.get(0).copied().unwrap_or(0),
                        data32.get(1).copied().unwrap_or(0),
                        data32.get(2).copied().unwrap_or(0),
                        data32.get(3).copied().unwrap_or(0),
                        data32.get(4).copied().unwrap_or(0),
                    ],
                    format: e.format,
                })
            }
            XEvent::MappingNotify(_) => Some(BackendEvent::MappingNotify),
            XEvent::Expose(e) => Some(BackendEvent::Expose {
                window: WindowId::X11(e.window as u64),
            }),
            _ => None,
        }
    }

    pub fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        let ev = self.conn.poll_for_event()?;
        Ok(ev.and_then(|e| self.map_event(e)))
    }
}

impl EventSource for X11EventSource {
    type Event = BackendEvent;
    type Metadata = ();
    type Ret = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn process_events<F>(
        &mut self,
        _readiness: Readiness,
        _token: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        loop {
            match self.poll_event() {
                Ok(Some(event)) => {
                    callback(event, &mut ());
                }
                Ok(None) => break,
                Err(e) => {
                    log::error!("X11 poll error: {:?}", e);
                    // 转换错误类型以满足 Send + Sync 约束
                    let err_msg = format!("X11 poll error: {}", e);
                    return Err(err_msg.into());
                }
            }
        }
        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        let raw_fd = self.conn.stream().as_raw_fd();
        unsafe {
            let fd = BorrowedFd::borrow_raw(raw_fd);
            poll.register(fd, Interest::READ, Mode::Level, token_factory.token())
        }
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        let raw_fd = self.conn.stream().as_raw_fd();
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        poll.reregister(fd, Interest::READ, Mode::Level, token_factory.token())
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        let raw_fd = self.conn.stream().as_raw_fd();
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        poll.unregister(fd)
    }
}
