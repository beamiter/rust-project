// src/backend/x11/backend.rs
use std::sync::{Arc, Mutex};
use log::info;
use x11rb::connection::Connection as _;
use x11rb::protocol::render::ConnectionExt as _;
use x11rb::protocol::xproto::{ColormapAlloc, Screen, Visualtype};
use x11rb::protocol::xproto::{ConnectionExt as _, VisualClass};
use x11rb::rust_connection::RustConnection;

use crate::backend::api::{
    Backend, Capabilities, EventSource, EwmhFacade, InputOps, KeyOps, OutputOps, PropertyOps,
    WindowId, WindowOps,
};
use crate::backend::api::{ColorAllocator, CursorProvider};
use crate::backend::x11::key_ops::X11KeyOps;

use super::{
    color::X11ColorAllocator, cursor::X11CursorProvider, event_source::X11EventSource,
    ewmh_facade::X11EwmhFacade, input_ops::X11InputOps, output_ops::X11OutputOps,
    property_ops::X11PropertyOps, window_ops::X11WindowOps, Atoms,
};

#[allow(dead_code)]
pub struct X11Backend {
    conn: Arc<RustConnection>,
    screen: Screen,
    root: WindowId,
    atoms: Atoms,

    caps: Capabilities,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    ewmh_facade: Option<Box<dyn EwmhFacade>>,

    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
    event_source: Box<dyn EventSource>,
}

impl X11Backend {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 连接 X11
        let (raw_conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)?;
        let conn = Arc::new(raw_conn);
        use x11rb::connection::Connection;
        let screen = conn.setup().roots[screen_num].clone();
        let root = WindowId(screen.root as u64);

        // Atoms
        let atoms = Atoms::new(conn.as_ref())?.reply()?;

        // 子服务
        let window_ops: Box<dyn WindowOps> = Box::new(X11WindowOps::new(conn.clone()));
        let input_ops: Box<dyn InputOps> = Box::new(X11InputOps::new(conn.clone(), screen.root));
        let property_ops: Box<dyn PropertyOps> =
            Box::new(X11PropertyOps::new(conn.clone(), atoms.clone()));
        let output_ops: Box<dyn OutputOps> = Box::new(X11OutputOps::new(
            conn.clone(),
            screen.root,
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ));
        let key_ops: Box<dyn KeyOps> = Box::new(X11KeyOps::new(conn.clone()));
        let ewmh_facade: Option<Box<dyn EwmhFacade>> = Some(Box::new(X11EwmhFacade::new(
            conn.clone(),
            root,
            atoms.clone(),
        )));
        let cursor_provider: Box<dyn CursorProvider> =
            Box::new(X11CursorProvider::new(conn.clone())?);
        let color_allocator: Box<dyn ColorAllocator> = Box::new(X11ColorAllocator::new(
            conn.clone(),
            screen.default_colormap,
        ));
        let event_source: Box<dyn EventSource> = Box::new(X11EventSource::new(conn.clone()));

        let caps = Capabilities {
            can_warp_pointer: true,
            has_active_window_prop: true,
            supports_client_list: true,
            ..Default::default()
        };

        Ok(Self {
            conn,
            screen,
            root,
            atoms,
            caps,
            window_ops,
            input_ops,
            property_ops,
            output_ops,
            key_ops,
            ewmh_facade,
            cursor_provider,
            color_allocator,
            event_source,
        })
    }

    pub fn atoms(&self) -> &Atoms {
        &self.atoms
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// 在 render_query_pict_formats 的结果中，查找给定 Visualid 对应的 Pictforminfo
    fn find_visual_format_local<'a>(
        &self,
        formats: &'a x11rb::protocol::render::QueryPictFormatsReply,
        visual: x11rb::protocol::xproto::Visualid,
    ) -> Option<&'a x11rb::protocol::render::Pictforminfo> {
        // 步骤：在 screens[..].depths[..].visuals[..] 里找到与 visual 匹配的 Pictvisual，
        // 再用它的 format 字段去 formats.formats 里找 Pictforminfo
        for screen in &formats.screens {
            for depth in &screen.depths {
                for v in &depth.visuals {
                    if v.visual == visual {
                        let fmt = v.format;
                        return formats.formats.iter().find(|f| f.id == fmt);
                    }
                }
            }
        }
        None
    }

    fn setup_argb_visual(
        &mut self,
        visualtype: &Visualtype,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let colormap_id = self.conn.generate_id()?;
        self.conn
            .create_colormap(
                ColormapAlloc::NONE,
                colormap_id,
                self.root.0 as u32,
                visualtype.visual_id,
            )?
            .check()?;

        Ok(())
    }
}

impl Backend for X11Backend {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn window_ops(&self) -> &dyn WindowOps {
        &*self.window_ops
    }
    fn input_ops(&self) -> &dyn InputOps {
        &*self.input_ops
    }
    fn input_ops_handle(&self) -> std::sync::Arc<std::sync::Mutex<dyn InputOps + Send>> {
        Arc::new(Mutex::new(super::input_ops::X11InputOps::new(
            self.conn.clone(),
            self.screen.root,
        )))
    }
    fn property_ops(&self) -> &dyn PropertyOps {
        &*self.property_ops
    }
    fn output_ops(&self) -> &dyn OutputOps {
        &*self.output_ops
    }
    fn key_ops(&self) -> &dyn KeyOps {
        &*self.key_ops
    }
    fn key_ops_mut(&mut self) -> &mut dyn KeyOps {
        &mut *self.key_ops
    }
    fn ewmh_facade(&self) -> Option<&dyn EwmhFacade> {
        self.ewmh_facade.as_deref()
    }

    fn cursor_provider(&mut self) -> &mut dyn CursorProvider {
        &mut *self.cursor_provider
    }
    fn color_allocator(&mut self) -> &mut dyn ColorAllocator {
        &mut *self.color_allocator
    }

    fn event_source(&mut self) -> &mut dyn EventSource {
        &mut *self.event_source
    }

    fn root_window(&self) -> WindowId {
        self.root
    }

    fn init_visual(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 查询 render pict formats
        let formats = self.conn.render_query_pict_formats()?.reply()?;
        // 优先寻找 32-bit TRUE_COLOR + 有 alpha 的 visual
        for depth in self.screen.allowed_depths.iter().cloned() {
            if depth.depth != 32 {
                continue;
            }
            for visualtype in &depth.visuals {
                if visualtype.class != VisualClass::TRUE_COLOR {
                    continue;
                }
                if let Some(info) = self.find_visual_format_local(&formats, visualtype.visual_id) {
                    if info.direct.alpha_mask != 0 {
                        return self.setup_argb_visual(visualtype);
                    }
                }
            }
        }
        // 没找到 32-bit ARGB，回退默认
        info!("[xinit_visual] No 32-bit ARGB visual found. Falling back to default.");
        Ok(())
    }
}
