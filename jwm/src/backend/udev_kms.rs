use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::Format as DmabufFormat;
use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::exporter::gbm::NodeFilter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata};
use smithay::backend::egl::context::ContextPriority;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::gles::GlesRenderbuffer;
use smithay::backend::renderer::{Bind, ExportMem, ImportAll, ImportMem, Offscreen};
use smithay::backend::session::Session;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::utils::{Buffer as BufferCoord, Size};
use smithay::desktop::layer_map_for_output;
use smithay::desktop::space::SurfaceTree;
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::output::{Mode as WlMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::channel::Sender;
use smithay::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay::reexports::drm::control::{Device as ControlDevice, ModeTypeFlags, connector, crtc};
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{DeviceFd, Physical, Point, Rectangle, Scale, Transform};
use smithay::wayland::compositor::{TraversalAction, with_states, with_surface_tree_downward};
use smithay::wayland::shell::xdg::SurfaceCachedState;
use smithay::wayland::shell::wlr_layer::Layer as WlrLayer;

use crate::backend::common_define::StdCursorKind;

use xcursor::{
    parser::{parse_xcursor, Image},
    CursorTheme,
};

smithay::backend::renderer::element::render_elements! {
    pub KmsRenderElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Solid=SolidColorRenderElement,
    Memory=MemoryRenderBufferRenderElement<R>,
}

pub(super) type KmsHandle = Rc<RefCell<KmsState>>;

struct KmsOutputState {
    crtc: crtc::Handle,
    mode_size: (i32, i32),
    origin: (i32, i32),

    output: Output,
    drm_output:
        DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>,

    frame_pending: bool,

    send_frame_callbacks: bool,
    frame_callback_roots: Vec<WlSurface>,
    frame_callback_throttle: Option<std::time::Duration>,
    frame_callback_visible: HashSet<wayland_server::Weak<WlSurface>>,

    surfaces_on_output: HashSet<wayland_server::Weak<WlSurface>>,
}

pub(super) struct KmsState {
    #[allow(dead_code)]
    dev_path: std::path::PathBuf,

    pub registration_token: Option<RegistrationToken>,

    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,

    #[allow(dead_code)]
    drm_output_manager: DrmOutputManager<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        (),
        DrmDeviceFd,
    >,
    #[allow(dead_code)]
    gbm: GbmDevice<DrmDeviceFd>,
    renderer: GlesRenderer,

    needs_render: bool,
    background_id: Id,

    cursor_theme: CursorTheme,
    cursor_size: u32,
    cursor_images: HashMap<String, Vec<Image>>,
    cursor_cache: HashMap<(StdCursorKind, u32), CursorBitmap>,

    cursor_fallback_body_ids: Vec<Id>,
    cursor_fallback_shadow_ids: Vec<Id>,

    pending_screenshot: Option<std::path::PathBuf>,

    /// Shared queue for pending screencopy frames (from wlr-screencopy-unstable-v1).
    screencopy_pending: Option<crate::backend::wayland_udev::screencopy::PendingScreencopyQueue>,

    outputs: Vec<KmsOutputState>,
}

#[derive(Clone)]
struct CursorBitmap {
    buffer: MemoryRenderBuffer,
    xhot: i32,
    yhot: i32,
}

// A tiny software cursor (pointer arrow) expressed as a list of rectangles.
// Coordinates are relative to the cursor hotspot (tip at 0,0).
const CURSOR_RECTS: &[(i32, i32, i32, i32)] = &[
    // Triangle head (11 scanlines)
    (0, 0, 1, 1),
    (0, 1, 2, 1),
    (0, 2, 3, 1),
    (0, 3, 4, 1),
    (0, 4, 5, 1),
    (0, 5, 6, 1),
    (0, 6, 7, 1),
    (0, 7, 8, 1),
    (0, 8, 9, 1),
    (0, 9, 10, 1),
    (0, 10, 11, 1),
    // Stem
    (3, 11, 3, 7),
    // Base
    (2, 18, 5, 2),
];

fn cursor_candidates(kind: StdCursorKind) -> &'static [&'static str] {
    match kind {
        StdCursorKind::LeftPtr => &["left_ptr", "default"],
        StdCursorKind::Hand => &["hand2", "hand1", "pointer", "default"],
        StdCursorKind::XTerm => &["xterm", "text", "default"],
        StdCursorKind::Watch => &["watch", "wait", "default"],
        StdCursorKind::Crosshair => &["crosshair", "default"],
        StdCursorKind::Fleur => &["fleur", "move", "default"],
        StdCursorKind::HDoubleArrow => &["sb_h_double_arrow", "h_double_arrow", "default"],
        StdCursorKind::VDoubleArrow => &["sb_v_double_arrow", "v_double_arrow", "default"],
        StdCursorKind::TopLeftCorner => &["top_left_corner", "nw-resize", "default"],
        StdCursorKind::TopRightCorner => &["top_right_corner", "ne-resize", "default"],
        StdCursorKind::BottomLeftCorner => &["bottom_left_corner", "sw-resize", "default"],
        StdCursorKind::BottomRightCorner => &["bottom_right_corner", "se-resize", "default"],
        StdCursorKind::Sizing => &["sizing", "default"],
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(super) enum KmsInitError {
    DeviceOpen(smithay::backend::session::libseat::Error),
    DrmInit(smithay::backend::drm::DrmError),
    GbmInit(std::io::Error),
    EglInit(smithay::backend::egl::Error),
    GlesInit(smithay::backend::renderer::gles::GlesError),
    NoConnector,
    NoCrtc,
    InitializeOutput(String),
}

impl std::fmt::Display for KmsInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KmsInitError::DeviceOpen(e) => write!(f, "libseat open failed: {e}"),
            KmsInitError::DrmInit(e) => write!(f, "drm init failed: {e}"),
            KmsInitError::GbmInit(e) => write!(f, "gbm init failed: {e}"),
            KmsInitError::EglInit(e) => write!(f, "egl init failed: {e}"),
            KmsInitError::GlesInit(e) => write!(f, "gles init failed: {e}"),
            KmsInitError::NoConnector => write!(f, "no connected drm connector found"),
            KmsInitError::NoCrtc => write!(f, "could not pick CRTC for connector"),
            KmsInitError::InitializeOutput(e) => write!(f, "initialize_output failed: {e}"),
        }
    }
}

impl std::error::Error for KmsInitError {}

impl KmsState {
    fn load_xcursor_images(&mut self, icon: &str) -> Option<&Vec<Image>> {
        if self.cursor_images.contains_key(icon) {
            return self.cursor_images.get(icon);
        }

        let images = self
            .cursor_theme
            .load_icon(icon)
            .and_then(|path| {
                let mut file = std::fs::File::open(path).ok()?;
                let mut data = Vec::new();
                file.read_to_end(&mut data).ok()?;
                parse_xcursor(&data)
            })
            .unwrap_or_default();

        self.cursor_images.insert(icon.to_string(), images);
        self.cursor_images.get(icon)
    }

    fn pick_nearest_image<'a>(images: &'a [Image], target_size: u32) -> Option<&'a Image> {
        let nearest = images
            .iter()
            .min_by_key(|img| (target_size as i32 - img.size as i32).abs())?;

        // If the cursor is animated, multiple frames share width/height.
        // We don't animate yet; pick the first frame of the nearest size.
        images
            .iter()
            .find(|img| img.width == nearest.width && img.height == nearest.height)
    }

    fn cursor_bitmap(&mut self, kind: StdCursorKind, scale: u32) -> Option<CursorBitmap> {
        let key = (kind, scale);
        if let Some(cached) = self.cursor_cache.get(&key) {
            return Some(cached.clone());
        }

        let target_size = self.cursor_size.saturating_mul(scale.max(1));

        for &name in cursor_candidates(kind) {
            let images = self.load_xcursor_images(name)?;
            if images.is_empty() {
                continue;
            }
            let img = Self::pick_nearest_image(images, target_size)?;
            if img.pixels_rgba.is_empty() || img.width == 0 || img.height == 0 {
                continue;
            }

            let buffer = MemoryRenderBuffer::from_slice(
                &img.pixels_rgba,
                Fourcc::Argb8888,
                (img.width as i32, img.height as i32),
                1,
                Transform::Normal,
                None,
            );
            let bitmap = CursorBitmap {
                buffer,
                xhot: img.xhot as i32,
                yhot: img.yhot as i32,
            };
            self.cursor_cache.insert(key, bitmap.clone());
            return Some(bitmap);
        }

        None
    }

    pub(super) fn request_render(&mut self) {
        self.needs_render = true;
    }

    /// Set the shared pending screencopy queue (called once after initialization).
    pub(super) fn set_screencopy_pending(&mut self, queue: crate::backend::wayland_udev::screencopy::PendingScreencopyQueue) {
        self.screencopy_pending = Some(queue);
    }

    /// Schedule a screenshot to be captured on the next render pass.
    pub(super) fn request_screenshot(&mut self, path: std::path::PathBuf) {
        self.pending_screenshot = Some(path);
        self.needs_render = true;
    }

    /// Render all elements to an offscreen buffer and save as PNG.
    /// Split out as a free-standing function so it can borrow `self.renderer`
    /// without conflicting with the mutable borrow on `self.outputs`.
    #[allow(dead_code)]
    fn capture_screenshot_offscreen_impl(
        renderer: &mut GlesRenderer,
        width: i32,
        height: i32,
        elements: &[KmsRenderElement<GlesRenderer>],
        path: &std::path::Path,
    ) {
        let size: Size<i32, BufferCoord> = (width, height).into();

        // 1. Create offscreen renderbuffer
        let mut renderbuffer: GlesRenderbuffer = match Offscreen::create_buffer(
            renderer,
            Fourcc::Abgr8888,
            size,
        ) {
            Ok(rb) => rb,
            Err(e) => {
                log::error!("[screenshot] create offscreen buffer failed: {e:?}");
                return;
            }
        };

        // 2. Bind the offscreen renderbuffer
        let mut target = match renderer.bind(&mut renderbuffer) {
            Ok(t) => t,
            Err(e) => {
                log::error!("[screenshot] bind offscreen failed: {e:?}");
                return;
            }
        };

        // 3. Render all elements using OutputDamageTracker
        let phys_size: smithay::utils::Size<i32, Physical> = (width, height).into();
        let mut damage_tracker = OutputDamageTracker::new(
            phys_size,
            Scale::from(1.0f64),
            Transform::Normal,
        );
        let clear_color = smithay::backend::renderer::Color32F::new(0.1, 0.15, 0.25, 1.0);
        if let Err(e) = damage_tracker.render_output(
            renderer,
            &mut target,
            0, // age=0 forces full redraw
            elements,
            clear_color,
        ) {
            log::error!("[screenshot] render_output failed: {e:?}");
            return;
        }

        // 4. Read pixels back via ExportMem
        let region = Rectangle::from_size(size);
        let mapping = match renderer.copy_framebuffer(&target, region, Fourcc::Abgr8888) {
            Ok(m) => m,
            Err(e) => {
                log::error!("[screenshot] copy_framebuffer failed: {e:?}");
                return;
            }
        };

        let pixels = match renderer.map_texture(&mapping) {
            Ok(p) => p,
            Err(e) => {
                log::error!("[screenshot] map_texture failed: {e:?}");
                return;
            }
        };

        // 5. Save as PNG (pixels are ABGR8888 / RGBA from GL perspective)
        if let Err(e) = save_rgba_png(path, width as u32, height as u32, pixels) {
            log::error!("[screenshot] save PNG failed: {e}");
        } else {
            log::info!("[screenshot] saved to {}", path.display());
        }
    }

    /// Fulfill pending wlr-screencopy copy requests for a given output.
    ///
    /// This renders the given elements to an offscreen buffer and copies the
    /// RGBA pixels into each waiting client's wl_shm buffer, then sends the
    /// `flags` + `ready` events on the screencopy frame.
    fn fulfill_screencopy_frames(
        renderer: &mut GlesRenderer,
        output: &Output,
        width: i32,
        height: i32,
        elements: &[KmsRenderElement<GlesRenderer>],
        pending: &crate::backend::wayland_udev::screencopy::PendingScreencopyQueue,
    ) {
        use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_frame_v1;
        use smithay::wayland::shm::with_buffer_contents_mut;

        let mut frames: Vec<crate::backend::wayland_udev::screencopy::PendingScreencopyFrame> = {
            let mut queue = pending.lock().unwrap();
            // Drain frames that match this output.
            let mut matching = Vec::new();
            let mut remaining = Vec::new();
            for f in queue.drain(..) {
                if f.output == *output {
                    matching.push(f);
                } else {
                    remaining.push(f);
                }
            }
            *queue = remaining;
            matching
        };

        if frames.is_empty() {
            return;
        }

        log::debug!(
            "[screencopy] fulfilling {} frames for output {}",
            frames.len(),
            output.name(),
        );

        // Render to offscreen buffer (same approach as screenshot capture).
        let size: Size<i32, BufferCoord> = (width, height).into();
        let mut renderbuffer: GlesRenderbuffer = match Offscreen::create_buffer(
            renderer,
            Fourcc::Abgr8888,
            size,
        ) {
            Ok(rb) => rb,
            Err(e) => {
                log::error!("[screencopy] create offscreen buffer failed: {e:?}");
                for f in &frames {
                    f.frame.failed();
                }
                return;
            }
        };

        let mut target = match renderer.bind(&mut renderbuffer) {
            Ok(t) => t,
            Err(e) => {
                log::error!("[screencopy] bind offscreen failed: {e:?}");
                for f in &frames {
                    f.frame.failed();
                }
                return;
            }
        };

        let phys_size: smithay::utils::Size<i32, Physical> = (width, height).into();
        let mut damage_tracker = OutputDamageTracker::new(
            phys_size,
            Scale::from(1.0f64),
            Transform::Normal,
        );
        let clear_color = smithay::backend::renderer::Color32F::new(0.1, 0.15, 0.25, 1.0);
        if let Err(e) = damage_tracker.render_output(
            renderer,
            &mut target,
            0,
            elements,
            clear_color,
        ) {
            log::error!("[screencopy] render_output failed: {e:?}");
            for f in &frames {
                f.frame.failed();
            }
            return;
        }

        // Read back pixels (ABGR8888 from GL).
        let region = Rectangle::from_size(size);
        let mapping = match renderer.copy_framebuffer(&target, region, Fourcc::Abgr8888) {
            Ok(m) => m,
            Err(e) => {
                log::error!("[screencopy] copy_framebuffer failed: {e:?}");
                for f in &frames {
                    f.frame.failed();
                }
                return;
            }
        };

        let pixels = match renderer.map_texture(&mapping) {
            Ok(p) => p,
            Err(e) => {
                log::error!("[screencopy] map_texture failed: {e:?}");
                for f in &frames {
                    f.frame.failed();
                }
                return;
            }
        };

        // GL gives us ABGR (little-endian RGBA bytes).
        // wl_shm ARGB8888 is native-endian: on little-endian it's [B, G, R, A] in memory.
        // GL ABGR8888 is [R, G, B, A] in memory.
        // We need to convert RGBA → BGRA (swap R and B channels).

        for frame_info in frames.drain(..) {
            let copy_result = with_buffer_contents_mut(&frame_info.buffer, |ptr, pool_len, buf_data| {
                let buf_offset = buf_data.offset as usize;
                let buf_stride = buf_data.stride as usize;
                let buf_h = buf_data.height as usize;
                let buf_w = buf_data.width as usize;

                // Source region
                let (src_x, src_y, src_w, src_h) = if let Some((rx, ry, rw, rh)) = frame_info.region {
                    (rx as usize, ry as usize, rw as usize, rh as usize)
                } else {
                    (0usize, 0usize, width as usize, height as usize)
                };

                let copy_h = src_h.min(buf_h);
                let copy_w = src_w.min(buf_w);
                let src_stride = width as usize * 4;

                for row in 0..copy_h {
                    let src_row = src_y + row;
                    if src_row >= height as usize {
                        break;
                    }
                    let src_row_start = src_row * src_stride + src_x * 4;
                    let dst_row_start = buf_offset + row * buf_stride;

                    if dst_row_start + copy_w * 4 > pool_len {
                        break;
                    }

                    for col in 0..copy_w {
                        let si = src_row_start + col * 4;
                        let di = dst_row_start + col * 4;
                        if si + 3 >= pixels.len() {
                            break;
                        }
                        // ABGR (GL) = [R, G, B, A] in memory → ARGB8888 (shm) = [B, G, R, A] in memory
                        unsafe {
                            *ptr.add(di) = pixels[si + 2]; // B
                            *ptr.add(di + 1) = pixels[si + 1]; // G
                            *ptr.add(di + 2) = pixels[si]; // R
                            *ptr.add(di + 3) = pixels[si + 3]; // A
                        }
                    }
                }
            });

            match copy_result {
                Ok(()) => {
                    // Send flags (no y-invert) then ready.
                    frame_info.frame.flags(zwlr_screencopy_frame_v1::Flags::empty());
                    // Timestamp: use current time.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    let tv_sec = now.as_secs();
                    let tv_nsec = now.subsec_nanos();
                    frame_info.frame.ready(
                        (tv_sec >> 32) as u32,
                        (tv_sec & 0xFFFFFFFF) as u32,
                        tv_nsec,
                    );
                    log::debug!("[screencopy] frame ready for output {}", output.name());
                }
                Err(e) => {
                    log::warn!("[screencopy] buffer access failed: {e:?}");
                    frame_info.frame.failed();
                }
            }
        }
    }

    pub(super) fn outputs(&self) -> Vec<Output> {
        self.outputs.iter().map(|o| o.output.clone()).collect()
    }

    pub(super) fn dmabuf_render_formats(&self) -> Vec<DmabufFormat> {
        self.renderer
            .egl_context()
            .dmabuf_render_formats()
            .iter()
            .copied()
            .collect()
    }

    pub(super) fn new(
        session: &mut LibSeatSession,
        dev_path: &Path,
        dev_id: u64,
        output_layout: &std::collections::HashMap<u64, (i32, i32)>,
        display_handle: &smithay::reexports::wayland_server::DisplayHandle,
        flush_tx: Sender<()>,
        flush_pending: Arc<AtomicBool>,
        event_loop_handle: LoopHandle<'static, crate::backend::wayland::state::JwmWaylandState>,
    ) -> Result<KmsHandle, KmsInitError> {
        let fd = session
            .open(
                dev_path,
                OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
            )
            .map_err(KmsInitError::DeviceOpen)?;
        let fd = DrmDeviceFd::new(DeviceFd::from(fd));

        let (drm, notifier) = DrmDevice::new(fd.clone(), true).map_err(KmsInitError::DrmInit)?;
        let gbm = GbmDevice::new(fd.clone()).map_err(KmsInitError::GbmInit)?;

        let display = unsafe { EGLDisplay::new(gbm.clone()).map_err(KmsInitError::EglInit)? };
        let context = EGLContext::new_with_priority(&display, ContextPriority::High)
            .map_err(KmsInitError::EglInit)?;
        let mut renderer = unsafe { GlesRenderer::new(context).map_err(KmsInitError::GlesInit)? };

        let allocator = GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        );
        let exporter = GbmFramebufferExporter::new(gbm.clone(), NodeFilter::None);

        let render_formats: FormatSet = renderer
            .egl_context()
            .dmabuf_render_formats()
            .iter()
            .copied()
            .collect();

        // Keep it simple and widely supported.
        let color_formats = [Fourcc::Argb8888, Fourcc::Xrgb8888];

        let mut drm_output_manager = DrmOutputManager::new(
            drm,
            allocator,
            exporter,
            Some(gbm.clone()),
            color_formats.into_iter(),
            render_formats,
        );

        #[derive(Clone)]
        struct PendingOutputInit {
            crtc: crtc::Handle,
            mode: smithay::reexports::drm::control::Mode,
            connector: connector::Handle,
            output: Output,
            mode_size: (i32, i32),
            origin: (i32, i32),
            frame_callback_throttle: Option<std::time::Duration>,
        }

        // Create outputs for all connected connectors with a usable (distinct) CRTC.
        let pending: Vec<PendingOutputInit> = {
            let drm_device = drm_output_manager.device();
            let res = drm_device.resource_handles().map_err(|e| {
                KmsInitError::InitializeOutput(format!("resource_handles failed: {e:?}"))
            })?;

            let mut used_crtcs: HashSet<crtc::Handle> = HashSet::new();
            let mut pending = Vec::new();

            for conn_handle in res.connectors() {
                let conn = drm_device.get_connector(*conn_handle, true).map_err(|e| {
                    KmsInitError::InitializeOutput(format!("get_connector failed: {e:?}"))
                })?;

                if conn.state() != connector::State::Connected || conn.modes().is_empty() {
                    continue;
                }

                let Some(crtc) = pick_crtc(drm_device, &res, &conn, &used_crtcs) else {
                    continue;
                };
                used_crtcs.insert(crtc);

                let mode = conn
                    .modes()
                    .iter()
                    .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
                    .copied()
                    .or_else(|| conn.modes().first().copied())
                    .unwrap();

                let wl_mode = WlMode::from(mode);
                let frame_callback_throttle = if wl_mode.refresh > 0 {
                    // Smithay's Mode.refresh is in mHz (e.g. 60000 == 60Hz).
                    Some(std::time::Duration::from_nanos(
                        (1_000_000_000u64.saturating_mul(1000)) / (wl_mode.refresh as u64),
                    ))
                } else {
                    None
                };

                let (phys_w, phys_h) = conn.size().unwrap_or((0, 0));
                let output_name = format!("{:?}-{}", conn.interface(), conn.interface_id());
                let output = Output::new(
                    output_name,
                    PhysicalProperties {
                        size: (phys_w as i32, phys_h as i32).into(),
                        subpixel: Subpixel::Unknown,
                        make: "Unknown".into(),
                        model: "Unknown".into(),
                        serial_number: "Unknown".into(),
                    },
                );

                for m in conn.modes() {
                    output.add_mode(WlMode::from(*m));
                }
                output.set_preferred(wl_mode);

                let key = (dev_id << 32) | (u32::from(*conn_handle) as u64);
                let (ox, oy) = output_layout.get(&key).copied().unwrap_or((0, 0));
                output.change_current_state(Some(wl_mode), None, None, Some((ox, oy).into()));

                pending.push(PendingOutputInit {
                    crtc,
                    mode,
                    connector: conn.handle(),
                    output,
                    mode_size: (mode.size().0 as i32, mode.size().1 as i32),
                    origin: (ox, oy),
                    frame_callback_throttle,
                });
            }

            pending
        };

        let render_elements: DrmOutputRenderElements<GlesRenderer, SolidColorRenderElement> =
            DrmOutputRenderElements::default();
        let mut outputs: Vec<KmsOutputState> = Vec::new();

        for p in pending {
            let _wl_output_global = p
                .output
                .create_global::<crate::backend::wayland::state::JwmWaylandState>(display_handle);

            let drm_output = drm_output_manager
                .lock()
                .initialize_output::<_, SolidColorRenderElement>(
                    p.crtc,
                    p.mode,
                    &[p.connector],
                    &p.output,
                    None,
                    &mut renderer,
                    &render_elements,
                )
                .map_err(|e| KmsInitError::InitializeOutput(format!("{e}")))?;

            outputs.push(KmsOutputState {
                crtc: p.crtc,
                mode_size: p.mode_size,
                origin: p.origin,
                output: p.output,
                drm_output,
                frame_pending: false,
                send_frame_callbacks: false,
                frame_callback_roots: Vec::new(),
                frame_callback_throttle: p.frame_callback_throttle,
                frame_callback_visible: HashSet::new(),
                surfaces_on_output: HashSet::new(),
            });
        }

        if outputs.is_empty() {
            return Err(KmsInitError::NoConnector);
        }

        let cursor_theme_name = std::env::var("XCURSOR_THEME")
            .ok()
            .unwrap_or_else(|| "default".into());
        let cursor_size = std::env::var("XCURSOR_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24u32);
        let cursor_theme = CursorTheme::load(&cursor_theme_name);

        let handle: KmsHandle = Rc::new(RefCell::new(KmsState {
            dev_path: dev_path.to_path_buf(),
            registration_token: None,
            flush_tx,
            flush_pending,
            drm_output_manager,
            gbm,
            renderer,
            needs_render: true,
            background_id: Id::new(),

            cursor_theme,
            cursor_size,
            cursor_images: HashMap::new(),
            cursor_cache: HashMap::new(),

            cursor_fallback_body_ids: (0..CURSOR_RECTS.len()).map(|_| Id::new()).collect(),
            cursor_fallback_shadow_ids: (0..CURSOR_RECTS.len()).map(|_| Id::new()).collect(),

            pending_screenshot: None,
            screencopy_pending: None,

            outputs,
        }));

        let handle_clone = handle.clone();
        let token = event_loop_handle
            .insert_source(notifier, move |event, metadata, _state| match event {
                DrmEvent::VBlank(crtc) => {
                    handle_clone.borrow_mut().on_vblank(crtc, metadata);
                }
                DrmEvent::Error(err) => {
                    log::warn!("drm event error: {err:?}");
                }
            })
            .expect("failed to register drm notifier");

        handle.borrow_mut().registration_token = Some(token);

        Ok(handle)
    }

    pub(super) fn render_if_needed(
        &mut self,
        state: &crate::backend::wayland::state::JwmWaylandState,
        cursor_kind: StdCursorKind,
    ) {
        if !self.needs_render {
            return;
        }

        for out_idx in 0..self.outputs.len() {
            let frame_pending = self.outputs[out_idx].frame_pending;
            if frame_pending {
                continue;
            }

            let scale: Scale<f64> = self.outputs[out_idx]
                .output
                .current_scale()
                .fractional_scale()
                .into();
            let (out_w, out_h) = self.outputs[out_idx].mode_size;
            let (ox, oy) = self.outputs[out_idx].origin;
            let output_rect_global = Rectangle::<i32, smithay::utils::Logical>::new(
                (ox, oy).into(),
                (out_w, out_h).into(),
            );

            // DrmOutput::render_frame expects elements in front-to-back order.
            // So: cursor/top-most surfaces first, solid background last.
            let mut elements: Vec<KmsRenderElement<GlesRenderer>> = Vec::new();

            // Cursor will be pushed FIRST (front-most).
            let cursor_x = state.pointer_location.x.round() as i32;
            let cursor_y = state.pointer_location.y.round() as i32;
            if cursor_x >= ox
                && cursor_y >= oy
                && cursor_x < (ox + out_w)
                && cursor_y < (oy + out_h)
            {
                // Approximate a cursor scale factor from the output scale.
                let cursor_scale = scale.x.max(1.0).ceil() as u32;
                let cursor_bitmap = self.cursor_bitmap(cursor_kind, cursor_scale);

                if let Some(bitmap) = cursor_bitmap.as_ref() {
                    let loc: Point<i32, Physical> = (
                        (cursor_x - ox) - bitmap.xhot,
                        (cursor_y - oy) - bitmap.yhot,
                    )
                        .into();
                    if let Ok(elem) = MemoryRenderBufferRenderElement::from_buffer(
                        &mut self.renderer,
                        loc.to_f64(),
                        &bitmap.buffer,
                        None,
                        None,
                        None,
                        Kind::Cursor,
                    ) {
                        elements.push(KmsRenderElement::Memory(elem));
                    }
                } else {
                    // Fallback: simple software pointer so we still have a visible cursor.
                    let base_x = cursor_x - ox;
                    let base_y = cursor_y - oy;

                    for (idx, (rx, ry, rw, rh)) in CURSOR_RECTS.iter().copied().enumerate() {
                        let geo: Rectangle<i32, Physical> = Rectangle::new(
                            (base_x + rx, base_y + ry).into(),
                            (rw, rh).into(),
                        );
                        let body = SolidColorRenderElement::new(
                            self.cursor_fallback_body_ids[idx].clone(),
                            geo,
                            0usize,
                            smithay::backend::renderer::Color32F::new(0.98, 0.98, 0.98, 1.0),
                            Kind::Cursor,
                        );
                        elements.push(KmsRenderElement::Solid(body));
                    }
                    for (idx, (rx, ry, rw, rh)) in CURSOR_RECTS.iter().copied().enumerate() {
                        let geo: Rectangle<i32, Physical> = Rectangle::new(
                            (base_x + rx + 1, base_y + ry + 1).into(),
                            (rw, rh).into(),
                        );
                        let shadow = SolidColorRenderElement::new(
                            self.cursor_fallback_shadow_ids[idx].clone(),
                            geo,
                            0usize,
                            smithay::backend::renderer::Color32F::new(0.0, 0.0, 0.0, 0.55),
                            Kind::Cursor,
                        );
                        elements.push(KmsRenderElement::Solid(shadow));
                    }
                }
            }

            let out = &mut self.outputs[out_idx];

            let mut visible_surfaces: HashSet<wayland_server::Weak<WlSurface>> = HashSet::new();
            let mut frame_roots: Vec<WlSurface> = Vec::new();

            // Layer surfaces above normal windows.
            {
                let map = layer_map_for_output(&out.output);
                for layer in [WlrLayer::Overlay, WlrLayer::Top] {
                    for ls in map.layers_on(layer) {
                        let Some(geo) = map.layer_geometry(ls) else {
                            continue;
                        };
                        let rect_global = Rectangle::<i32, smithay::utils::Logical>::new(
                            (ox + geo.loc.x, oy + geo.loc.y).into(),
                            geo.size,
                        );
                        if !rect_global.overlaps(output_rect_global) {
                            continue;
                        }

                        let surface = ls.wl_surface().clone();
                        frame_roots.push(surface.clone());

                        with_surface_tree_downward(
                            &surface,
                            (),
                            |_, _, _| TraversalAction::DoChildren(()),
                            |child_surface, child_states, _| {
                                let data =
                                    child_states.data_map.get::<RendererSurfaceStateUserData>();
                                let Some(data) = data else {
                                    return;
                                };
                                if data.lock().unwrap().view().is_some() {
                                    out.output.enter(child_surface);
                                    visible_surfaces.insert(child_surface.downgrade());
                                }
                            },
                            |_, _, _| true,
                        );

                        let location: Point<i32, Physical> = (geo.loc.x, geo.loc.y).into();
                        let tree = SurfaceTree::from_surface(&surface);
                        let layer_elements: Vec<KmsRenderElement<GlesRenderer>> =
                            AsRenderElements::<GlesRenderer>::render_elements(
                                &tree,
                                &mut self.renderer,
                                location,
                                scale,
                                1.0,
                            );
                        elements.extend(layer_elements);
                    }
                }
            }

            for win in state.window_stack.iter().rev() {
                if !state.mapped_windows.contains(win) {
                    continue;
                }
                let Some(geo) = state.window_geometry.get(win) else {
                    continue;
                };
                let Some(surface) = state.surface_for_window(*win) else {
                    continue;
                };

                // Many toolkits set an xdg_surface window-geometry with a non-zero loc (e.g. to
                // exclude client-side shadows). `state.window_geometry` tracks the window-geometry
                // origin in global coords, but the wl_surface buffer origin must be shifted by
                // -committed_geometry.loc to visually align.
                let (toplevel_off_x, toplevel_off_y) = with_states(&surface, |states| {
                    let mut cached = states.cached_state.get::<SurfaceCachedState>();
                    cached
                        .current()
                        .geometry
                        .map(|r| (r.loc.x, r.loc.y))
                        .unwrap_or((0, 0))
                });

                // Render any popups belonging to this toplevel above it (but below cursor).
                // Popups are separate wl_surfaces, not subsurfaces, so they won't appear in the
                // parent's SurfaceTree.
                for (popup_surface, popup_rect) in state.popup_rects_for_toplevel(*win) {
                    if !popup_rect.overlaps(output_rect_global) {
                        continue;
                    }

                    frame_roots.push(popup_surface.clone());

                    with_surface_tree_downward(
                        &popup_surface,
                        (),
                        |_, _, _| TraversalAction::DoChildren(()),
                        |child_surface, child_states, _| {
                            let data = child_states.data_map.get::<RendererSurfaceStateUserData>();
                            let Some(data) = data else {
                                return;
                            };
                            if data.lock().unwrap().view().is_some() {
                                out.output.enter(child_surface);
                                visible_surfaces.insert(child_surface.downgrade());
                            }
                        },
                        |_, _, _| true,
                    );

                    let (popup_off_x, popup_off_y) = with_states(&popup_surface, |states| {
                        let mut cached = states.cached_state.get::<SurfaceCachedState>();
                        cached
                            .current()
                            .geometry
                            .map(|r| (r.loc.x, r.loc.y))
                            .unwrap_or((0, 0))
                    });

                    let location: Point<i32, Physical> = (
                        popup_rect.loc.x - ox - popup_off_x,
                        popup_rect.loc.y - oy - popup_off_y,
                    )
                        .into();
                    let tree = SurfaceTree::from_surface(&popup_surface);
                    let popup_elements: Vec<KmsRenderElement<GlesRenderer>> =
                        AsRenderElements::<GlesRenderer>::render_elements(
                            &tree,
                            &mut self.renderer,
                            location,
                            scale,
                            1.0,
                        );
                    elements.extend(popup_elements);
                }

                let win_rect = Rectangle::<i32, smithay::utils::Logical>::new(
                    (geo.x, geo.y).into(),
                    (geo.w as i32, geo.h as i32).into(),
                );
                if !win_rect.overlaps(output_rect_global) {
                    continue;
                }

                frame_roots.push(surface.clone());

                with_surface_tree_downward(
                    &surface,
                    (),
                    |_, _, _| TraversalAction::DoChildren(()),
                    |child_surface, child_states, _| {
                        let data = child_states.data_map.get::<RendererSurfaceStateUserData>();
                        let Some(data) = data else {
                            return;
                        };
                        if data.lock().unwrap().view().is_some() {
                            out.output.enter(child_surface);
                            visible_surfaces.insert(child_surface.downgrade());
                        }
                    },
                    |_, _, _| true,
                );

                let location: Point<i32, Physical> = (
                    geo.x - ox - toplevel_off_x,
                    geo.y - oy - toplevel_off_y,
                )
                    .into();
                let tree = SurfaceTree::from_surface(&surface);
                let window_elements: Vec<KmsRenderElement<GlesRenderer>> =
                    AsRenderElements::<GlesRenderer>::render_elements(
                        &tree,
                        &mut self.renderer,
                        location,
                        scale,
                        1.0,
                    );
                elements.extend(window_elements);

                // Render window borders (server-side decorations for tiling WM).
                // The geometry `geo` represents the client content area. Borders are drawn
                // outside this area. The full window extent is
                //   (x - border, y - border, w + 2*border, h + 2*border).
                // Borders are rendered behind the window surface (after it in the front-to-back
                // element list) so the surface covers the inner area naturally.
                if geo.border > 0 {
                    let bw = geo.border as i32;
                    let [cr, cg, cb, ca] = state
                        .window_border_color
                        .get(win)
                        .copied()
                        .unwrap_or([0.3, 0.3, 0.35, 1.0]);
                    let border_color = smithay::backend::renderer::Color32F::new(cr, cg, cb, ca);

                    // Draw as a single solid rect the size of the full window (content + borders),
                    // placed behind the surface. The surface (already in the element list above)
                    // will overdraw the inner area, leaving only the border visible.
                    let full_geo: Rectangle<i32, Physical> = Rectangle::new(
                        (geo.x - ox - bw, geo.y - oy - bw).into(),
                        (geo.w as i32 + 2 * bw, geo.h as i32 + 2 * bw).into(),
                    );
                    elements.push(KmsRenderElement::Solid(SolidColorRenderElement::new(
                        Id::new(),
                        full_geo,
                        0usize,
                        border_color,
                        Kind::Unspecified,
                    )));
                }
            }

            // Layer surfaces below normal windows.
            {
                let map = layer_map_for_output(&out.output);
                for layer in [WlrLayer::Bottom, WlrLayer::Background] {
                    for ls in map.layers_on(layer) {
                        let Some(geo) = map.layer_geometry(ls) else {
                            continue;
                        };
                        let rect_global = Rectangle::<i32, smithay::utils::Logical>::new(
                            (ox + geo.loc.x, oy + geo.loc.y).into(),
                            geo.size,
                        );
                        if !rect_global.overlaps(output_rect_global) {
                            continue;
                        }

                        let surface = ls.wl_surface().clone();
                        frame_roots.push(surface.clone());

                        with_surface_tree_downward(
                            &surface,
                            (),
                            |_, _, _| TraversalAction::DoChildren(()),
                            |child_surface, child_states, _| {
                                let data =
                                    child_states.data_map.get::<RendererSurfaceStateUserData>();
                                let Some(data) = data else {
                                    return;
                                };
                                if data.lock().unwrap().view().is_some() {
                                    out.output.enter(child_surface);
                                    visible_surfaces.insert(child_surface.downgrade());
                                }
                            },
                            |_, _, _| true,
                        );

                        let location: Point<i32, Physical> = (geo.loc.x, geo.loc.y).into();
                        let tree = SurfaceTree::from_surface(&surface);
                        let layer_elements: Vec<KmsRenderElement<GlesRenderer>> =
                            AsRenderElements::<GlesRenderer>::render_elements(
                                &tree,
                                &mut self.renderer,
                                location,
                                scale,
                                1.0,
                            );
                        elements.extend(layer_elements);
                    }
                }
            }

            for gone in out.surfaces_on_output.difference(&visible_surfaces) {
                if let Ok(surf) = gone.upgrade() {
                    out.output.leave(&surf);
                }
            }
            out.surfaces_on_output = visible_surfaces.clone();
            // Drop the `out` borrow so we can access other `self` fields below.
            // drop(out);
            let _ = out;

            // Solid background LAST (back-most). Keep it opaque so we don't leak the previous
            // framebuffer contents on tty (which can look like a solid blue screen).
            let bg_geo = Rectangle::<i32, Physical>::from_size((out_w, out_h).into());
            let bg = SolidColorRenderElement::new(
                self.background_id.clone(),
                bg_geo,
                0usize,
                smithay::backend::renderer::Color32F::new(0.1, 0.15, 0.25, 1.0),
                Kind::Unspecified,
            );
            elements.push(KmsRenderElement::Solid(bg));

            // ── Screenshot capture (offscreen render) ───────────────────────
            if out_idx == 0 {
                if let Some(screenshot_path) = self.pending_screenshot.take() {
                    Self::capture_screenshot_offscreen_impl(
                        &mut self.renderer,
                        out_w,
                        out_h,
                        &elements,
                        &screenshot_path,
                    );
                }
            }

            // ── wlr-screencopy fulfillment ──────────────────────────────────
            if let Some(ref pending_queue) = self.screencopy_pending {
                let output_ref = &self.outputs[out_idx].output;
                Self::fulfill_screencopy_frames(
                    &mut self.renderer,
                    output_ref,
                    out_w,
                    out_h,
                    &elements,
                    pending_queue,
                );
            }

            // Re-borrow for render_frame + queue_frame.
            let out = &mut self.outputs[out_idx];

            match out.drm_output.render_frame(
                &mut self.renderer,
                &elements,
                smithay::backend::renderer::Color32F::new(0.0, 0.0, 0.0, 1.0),
                FrameFlags::DEFAULT,
            ) {
                Ok(res) => {
                    if res.is_empty {
                        out.send_frame_callbacks = false;
                        out.frame_callback_roots.clear();
                        out.frame_callback_visible.clear();
                        continue;
                    }

                    if let Err(err) = out.drm_output.queue_frame(()) {
                        log::warn!("drm queue_frame failed: {err:?}");

                        // If we started while not being DRM master (e.g. GNOME was active),
                        // switching VTs later can make us eligible to become master. Try to
                        // (re-)activate the DRM backend so subsequent frames can be queued.
                        match self.drm_output_manager.lock().activate(false) {
                            Ok(_) => {
                                log::info!("drm backend activated after queue_frame failure; will retry rendering");
                                self.needs_render = true;
                            }
                            Err(act_err) => {
                                log::warn!("drm backend activate failed after queue_frame failure: {act_err:?}");
                            }
                        }
                    } else {
                        out.frame_pending = true;
                        out.send_frame_callbacks = true;
                        out.frame_callback_roots = frame_roots;
                        out.frame_callback_visible = visible_surfaces;
                    }
                }
                Err(err) => {
                    log::warn!("drm render_frame failed: {err:?}");

                    match self.drm_output_manager.lock().activate(false) {
                        Ok(_) => {
                            log::info!("drm backend activated after render_frame failure; will retry rendering");
                            self.needs_render = true;
                        }
                        Err(act_err) => {
                            log::warn!("drm backend activate failed after render_frame failure: {act_err:?}");
                        }
                    }
                }
            }
        }

        self.needs_render = false;

        // Rendering can enqueue Wayland events (enter/leave, etc.).
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }

    pub(super) fn on_vblank(
        &mut self,
        crtc: crtc::Handle,
        _metadata: &mut Option<DrmEventMetadata>,
    ) {
        let Some(out) = self.outputs.iter_mut().find(|o| o.crtc == crtc) else {
            return;
        };

        if let Err(err) = out.drm_output.frame_submitted() {
            log::debug!("drm frame_submitted error: {err:?}");
        }
        out.frame_pending = false;

        if out.send_frame_callbacks {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO);

            let throttle = out.frame_callback_throttle;
            let output = out.output.clone();
            let visible = out.frame_callback_visible.clone();

            for root in &out.frame_callback_roots {
                send_frames_surface_tree(root, &output, now, throttle, |surface, states| {
                    let data = states.data_map.get::<RendererSurfaceStateUserData>();
                    let Some(data) = data else {
                        return None;
                    };
                    if data.lock().unwrap().view().is_none() {
                        return None;
                    }
                    if visible.contains(&surface.downgrade()) {
                        Some(output.clone())
                    } else {
                        None
                    }
                });
            }

            out.send_frame_callbacks = false;
            out.frame_callback_roots.clear();
            out.frame_callback_visible.clear();

            // Frame callbacks are Wayland events; flush them promptly.
            if !self.flush_pending.swap(true, Ordering::SeqCst) {
                let _ = self.flush_tx.send(());
            }
        }
    }
}

fn pick_crtc(
    drm_device: &DrmDevice,
    res: &smithay::reexports::drm::control::ResourceHandles,
    conn: &connector::Info,
    used_crtcs: &HashSet<crtc::Handle>,
) -> Option<crtc::Handle> {
    // Prefer encoder's current CRTC, otherwise pick the first possible.
    for enc_handle in conn.encoders() {
        let enc = drm_device.get_encoder(*enc_handle).ok()?;
        if let Some(crtc) = enc.crtc() {
            if !used_crtcs.contains(&crtc) {
                return Some(crtc);
            }
        }

        let possible = enc.possible_crtcs();
        for crtc in res.filter_crtcs(possible) {
            if !used_crtcs.contains(&crtc) {
                return Some(crtc);
            }
        }
    }

    None
}

/// Save raw RGBA pixel data as a PNG file.
///
/// OpenGL `glReadPixels` with `GL_RGBA` returns rows bottom-to-top, so we flip
/// the rows before encoding.
fn save_rgba_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let w = BufWriter::new(file);

    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;

    let stride = (width * 4) as usize;
    // Flip vertically (GL origin is bottom-left, PNG is top-left)
    let mut flipped = Vec::with_capacity(pixels.len());
    for row in (0..height as usize).rev() {
        let start = row * stride;
        flipped.extend_from_slice(&pixels[start..start + stride]);
    }

    writer.write_image_data(&flipped)?;
    Ok(())
}
