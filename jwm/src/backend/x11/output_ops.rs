// src/backend/x11/output_ops.rs
use crate::backend::api::{OutputInfo, OutputOps, ScreenInfo};
use crate::backend::common_define::OutputId;
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

    fn output_at(&self, x: i32, y: i32) -> Option<OutputId> {
        let outputs = self.enumerate_outputs();
        for output in outputs {
            if x >= output.x
                && x < output.x + output.width
                && y >= output.y
                && y < output.y + output.height
            {
                return Some(output.id);
            }
        }
        None
    }

    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
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
                                    id: OutputId(i as u64),
                                    name: format!("Monitor-{}", i),
                                    x: m.x as i32,
                                    y: m.y as i32,
                                    width: m.width as i32,
                                    height: m.height as i32,
                                    scale: 1.0,
                                    refresh_rate: 60000, // 60Hz
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
                            id: OutputId(i as u64),
                            name: format!("CRTC-{}", i),
                            x: ci.x as i32,
                            y: ci.y as i32,
                            width: ci.width as i32,
                            height: ci.height as i32,
                            scale: 1.0,
                            refresh_rate: 60000,
                        });
                    }
                }
            }
            return out;
        }
        vec![OutputInfo {
            id: OutputId(0),
            name: "Default".to_string(),
            x: 0,
            y: 0,
            width: self.sw,
            height: self.sh,
            scale: 1.0,
            refresh_rate: 60000,
        }]
    }
}
