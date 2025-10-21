// src/backend/x11/output_ops.rs
use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;

pub struct X11OutputOps<C: Connection> {
    conn: Arc<C>,
    root: u32,
    sw: i32,
    sh: i32,
}

impl<C: Connection> X11OutputOps<C> {
    pub fn new(conn: Arc<C>, root: u32, sw: i32, sh: i32) -> Self {
        Self { conn, root, sw, sh }
    }
}

impl<C: Connection + Send + Sync + 'static> OutputOps for X11OutputOps<C> {
    fn screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: self.sw,
            height: self.sh,
        }
    }

    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        // 尝试 RandR 1.5: GetMonitors
        if let Ok(ver) = self.conn.randr_query_version(1, 5) {
            if let Ok(v) = ver.reply() {
                if (v.major_version > 1) || (v.major_version == 1 && v.minor_version >= 5) {
                    if let Ok(reply) = self
                        .conn
                        .randr_get_monitors(self.root, true)
                        .and_then(|c| Ok(c.reply()))
                    {
                        let mut out = Vec::new();
                        for (i, m) in reply.unwrap().monitors.into_iter().enumerate() {
                            if m.width > 0 && m.height > 0 {
                                out.push(OutputInfo {
                                    id: i as i32,
                                    x: m.x as i32,
                                    y: m.y as i32,
                                    width: m.width as i32,
                                    height: m.height as i32,
                                });
                            }
                        }
                        if !out.is_empty() {
                            return out;
                        }
                    }
                }
            }
        }

        // 回退 RandR 1.2：resources + crtc_info
        if let Ok(resources) = self
            .conn
            .randr_get_screen_resources(self.root)
            .and_then(|c| Ok(c.reply()))
        {
            let mut out = Vec::new();
            for (i, crtc) in resources.unwrap().crtcs.into_iter().enumerate() {
                if let Ok(ci) = self
                    .conn
                    .randr_get_crtc_info(crtc, 0)
                    .and_then(|c| Ok(c.reply()))
                {
                    let ci = ci.unwrap();
                    if ci.width > 0 && ci.height > 0 {
                        out.push(OutputInfo {
                            id: i as i32,
                            x: ci.x as i32,
                            y: ci.y as i32,
                            width: ci.width as i32,
                            height: ci.height as i32,
                        });
                    }
                }
            }
            return out;
        }
        // 如果都不可用，回退一个全屏输出
        vec![OutputInfo {
            id: 0,
            x: 0,
            y: 0,
            width: self.sw,
            height: self.sh,
        }]
    }
}
