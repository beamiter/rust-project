// src/backend/x11/input_ops.rs
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;

pub struct X11InputOps<C: Connection> {
    conn: Arc<C>,
    root: Window,
}

impl<C: Connection> X11InputOps<C> {
    pub fn new(conn: Arc<C>, root: Window) -> Self {
        Self { conn, root }
    }
}

impl<C: Connection + Send + Sync + 'static> X11InputOps<C> {
    // 抓取指针，返回 GrabStatus
    pub fn grab_pointer(
        &self,
        event_mask: EventMask,
        cursor: Option<Cursor>,
    ) -> Result<GrabStatus, Box<dyn std::error::Error>> {
        let cursor_id = cursor.unwrap_or(0);
        let reply = self
            .conn
            .grab_pointer(
                false,
                self.root,
                event_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                0u32,
                cursor_id,
                0u32,
            )?
            .reply()?;
        Ok(reply.status)
    }

    pub fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.ungrab_pointer(0u32)?.check()?;
        Ok(())
    }

    pub fn warp_pointer_to_window(
        &self,
        window: Window,
        x: i16,
        y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .warp_pointer(0u32, window, 0, 0, 0, 0, x, y)?
            .check()?;
        Ok(())
    }

    pub fn allow_events(&self, mode: Allow, time: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.allow_events(mode, time)?.check()?;
        Ok(())
    }

    pub fn query_pointer(&self) -> Result<QueryPointerReply, Box<dyn std::error::Error>> {
        Ok(self.conn.query_pointer(self.root)?.reply()?)
    }

    pub fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.conn.flush()?;
        Ok(())
    }

    fn keycode_to_keysym(&self, keycode: u8) -> Result<u32, Box<dyn std::error::Error>> {
        let mapping = self.conn.get_keyboard_mapping(keycode, 1)?.reply()?;
        Ok(mapping.keysyms.get(0).copied().unwrap_or(0))
    }

    /// 通用拖拽循环
    /// - grab_mask: 抓取的事件掩码（通常是 BUTTON_PRESS|BUTTON_RELEASE|POINTER_MOTION）
    /// - cursor: 拖拽时光标（可选）
    /// - warp_to: 可选，相对窗口坐标（例如 resize 时定位到右下角）
    /// - target_window: 关联窗口，用于 Destroy/Unmap 的中止条件
    /// - on_motion: MotionNotify 时的回调（由上层实现移动/缩放逻辑）
    pub fn drag_loop<F>(
        &self,
        grab_mask: EventMask,
        cursor: Option<Cursor>,
        warp_to: Option<(i16, i16)>,
        target_window: Window,
        mut on_motion: F,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnMut(&MotionNotifyEvent) -> Result<(), Box<dyn std::error::Error>>,
    {
        // 抓指针
        match self.grab_pointer(grab_mask, cursor) {
            Ok(GrabStatus::SUCCESS) => {}
            Ok(status) => {
                let status_str = match status {
                    GrabStatus::ALREADY_GRABBED => "AlreadyGrabbed",
                    GrabStatus::FROZEN => "Frozen",
                    GrabStatus::INVALID_TIME => "InvalidTime",
                    GrabStatus::NOT_VIEWABLE => "NotViewable",
                    _ => "Unknown",
                };
                return Err(format!("Failed to grab pointer: {}", status_str).into());
            }
            Err(e) => return Err(e),
        }

        // 可选 warp
        if let Some((wx, wy)) = warp_to {
            self.warp_pointer_to_window(target_window, wx, wy)?;
        }
        self.flush()?;

        let mut last_motion_time: u32 = 0;

        loop {
            match self.conn.poll_for_event()? {
                Some(Event::MotionNotify(e)) => {
                    // ~16ms 节流
                    if e.time.wrapping_sub(last_motion_time) <= 16 {
                        continue;
                    }
                    last_motion_time = e.time;
                    on_motion(&e)?;
                }
                Some(Event::ButtonRelease(_)) => {
                    // 松开按键，结束
                    break;
                }
                Some(Event::KeyPress(e)) => {
                    // ESC 取消
                    const XK_ESCAPE: u32 = 0xff1b; // x11::keysym::XK_Escape
                    let ks = self.keycode_to_keysym(e.detail)?;
                    if ks == XK_ESCAPE {
                        break;
                    }
                }
                Some(Event::DestroyNotify(e)) => {
                    if e.window == target_window {
                        break;
                    }
                }
                Some(Event::UnmapNotify(e)) => {
                    if e.window == target_window {
                        break;
                    }
                }
                Some(_other) => {
                    // 其他事件忽略（如需，后续可增加 on_event 回调转发）
                }
                None => {
                    // 暂无事件，稍微退避，避免busy loop
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }

        Ok(())
    }
}
