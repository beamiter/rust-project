use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use smithay::backend::allocator::Fourcc;
use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::exporter::gbm::NodeFilter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata};
use smithay::backend::egl::context::ContextPriority;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::{ImportAll, ImportMem};
use smithay::backend::session::Session;
use smithay::backend::session::libseat::LibSeatSession;
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
use smithay::utils::{DeviceFd, Physical, Point, Rectangle, Scale};
use smithay::wayland::compositor::{TraversalAction, with_surface_tree_downward};
use smithay::wayland::shell::wlr_layer::Layer as WlrLayer;

smithay::backend::renderer::element::render_elements! {
    pub KmsRenderElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Solid=SolidColorRenderElement,
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

    cursor_id: Id,
    cursor_size: i32,

    outputs: Vec<KmsOutputState>,
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
    pub(super) fn request_render(&mut self) {
        self.needs_render = true;
    }

    pub(super) fn outputs(&self) -> Vec<Output> {
        self.outputs.iter().map(|o| o.output.clone()).collect()
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

            cursor_id: Id::new(),
            cursor_size: 12,

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
    ) {
        if !self.needs_render {
            return;
        }

        for out in &mut self.outputs {
            if out.frame_pending {
                continue;
            }

            let scale: Scale<f64> = out.output.current_scale().fractional_scale().into();
            let (out_w, out_h) = out.mode_size;
            let (ox, oy) = out.origin;
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
                let cursor_geo: Rectangle<i32, Physical> = Rectangle::new(
                    (cursor_x - ox, cursor_y - oy).into(),
                    (self.cursor_size, self.cursor_size).into(),
                );
                let cursor = SolidColorRenderElement::new(
                    self.cursor_id.clone(),
                    cursor_geo,
                    0usize,
                    smithay::backend::renderer::Color32F::new(0.95, 0.95, 0.95, 1.0),
                    Kind::Cursor,
                );
                elements.push(KmsRenderElement::Solid(cursor));
            }

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

                    let location: Point<i32, Physical> =
                        (popup_rect.loc.x - ox, popup_rect.loc.y - oy).into();
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

                let location: Point<i32, Physical> = (geo.x - ox, geo.y - oy).into();
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
