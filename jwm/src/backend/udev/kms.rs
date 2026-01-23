use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::FrameFlags;
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata};
use smithay::backend::egl::context::ContextPriority;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Id, Kind};
use smithay::backend::renderer::{ImportAll, ImportMem};
use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::Session;
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::desktop::space::SurfaceTree;
use smithay::output::{Mode as WlMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay::reexports::drm::control::{connector, crtc, Device as ControlDevice, ModeTypeFlags};
use smithay::backend::drm::exporter::gbm::NodeFilter;
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{DeviceFd, Physical, Point, Rectangle, Scale};
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};

smithay::backend::renderer::element::render_elements! {
    pub KmsRenderElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Solid=SolidColorRenderElement,
}

pub type KmsHandle = Rc<RefCell<KmsState>>;

pub struct KmsState {
    #[allow(dead_code)]
    dev_path: std::path::PathBuf,

    pub registration_token: Option<RegistrationToken>,

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

    crtc: crtc::Handle,
    mode_size: (i32, i32),
    #[allow(dead_code)]
    output: Output,
    drm_output: DrmOutput<
        GbmAllocator<DrmDeviceFd>,
        GbmFramebufferExporter<DrmDeviceFd>,
        (),
        DrmDeviceFd,
    >,

    needs_render: bool,
    frame_pending: bool,
    send_frame_callbacks: bool,
    frame_callback_roots: Vec<WlSurface>,
    frame_callback_throttle: Option<std::time::Duration>,
    frame_callback_visible: HashSet<wayland_server::Weak<WlSurface>>,
    background_id: Id,

    cursor_id: Id,
    cursor_size: i32,

    surfaces_on_output: HashSet<wayland_server::Weak<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>>,
}

#[derive(Debug)]
pub enum KmsInitError {
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
    pub fn request_render(&mut self) {
        self.needs_render = true;
    }

    pub fn new(
        session: &mut LibSeatSession,
        dev_path: &Path,
        display_handle: &smithay::reexports::wayland_server::DisplayHandle,
        event_loop_handle: LoopHandle<'static, crate::backend::udev::wayland::JwmWaylandState>,
    ) -> Result<KmsHandle, KmsInitError> {
        let fd = session.open(
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

        // Pick the first connected connector and a usable CRTC.
        let drm_device = drm_output_manager.device();
        let res = drm_device
            .resource_handles()
            .map_err(|e| KmsInitError::InitializeOutput(format!("resource_handles failed: {e:?}")))?;

        let mut chosen: Option<(connector::Info, crtc::Handle)> = None;
        for conn_handle in res.connectors() {
            let conn = drm_device
                .get_connector(*conn_handle, true)
                .map_err(|e| KmsInitError::InitializeOutput(format!("get_connector failed: {e:?}")))?;

            if conn.state() != connector::State::Connected || conn.modes().is_empty() {
                continue;
            }

            if let Some(crtc) = pick_crtc(drm_device, &res, &conn) {
                chosen = Some((conn, crtc));
                break;
            }
        }

        let (connector, crtc) = chosen.ok_or(KmsInitError::NoConnector)?;

        let mode = connector
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .copied()
            .or_else(|| connector.modes().first().copied())
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
        let (phys_w, phys_h) = connector.size().unwrap_or((0, 0));
        let output_name = format!("{:?}-{}", connector.interface(), connector.interface_id());
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
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), None, None, Some((0, 0).into()));

        // Advertise wl_output to clients.
        let _wl_output_global = output.create_global::<crate::backend::udev::wayland::JwmWaylandState>(display_handle);

        // We only render a solid color for now.
        let render_elements: DrmOutputRenderElements<GlesRenderer, SolidColorRenderElement> =
            DrmOutputRenderElements::default();

        let drm_output = drm_output_manager
            .lock()
            .initialize_output::<_, SolidColorRenderElement>(
                crtc,
                mode,
                &[connector.handle()],
                &output,
                None,
                &mut renderer,
                &render_elements,
            )
            .map_err(|e| KmsInitError::InitializeOutput(format!("{e}")))?;

        let handle: KmsHandle = Rc::new(RefCell::new(KmsState {
            dev_path: dev_path.to_path_buf(),
            registration_token: None,
            drm_output_manager,
            gbm,
            renderer,
            crtc,
            mode_size: (mode.size().0 as i32, mode.size().1 as i32),
            output,
            drm_output,
            needs_render: true,
            frame_pending: false,
            send_frame_callbacks: false,
            frame_callback_roots: Vec::new(),
            frame_callback_throttle,
            frame_callback_visible: HashSet::new(),
            background_id: Id::new(),

            cursor_id: Id::new(),
            cursor_size: 12,

            surfaces_on_output: HashSet::new(),
        }));

        let handle_clone = handle.clone();
        let token = event_loop_handle
            .insert_source(notifier, move |event, metadata, _state| {
                match event {
                    DrmEvent::VBlank(crtc) => {
                        handle_clone.borrow_mut().on_vblank(crtc, metadata);
                    }
                    DrmEvent::Error(err) => {
                        tracing::warn!("drm event error: {err:?}");
                    }
                }
            })
            .expect("failed to register drm notifier");

        handle.borrow_mut().registration_token = Some(token);

        Ok(handle)
    }

    pub fn render_if_needed(&mut self, state: &crate::backend::udev::wayland::JwmWaylandState) {
        if !self.needs_render || self.frame_pending {
            return;
        }

        let scale: Scale<f64> = self.output.current_scale().fractional_scale().into();

        // DrmOutput::render_frame expects elements in front-to-back order.
        // So: top-most windows first, background last.
        let mut elements: Vec<KmsRenderElement<GlesRenderer>> = Vec::new();

        // Minimal visible cursor: a small solid square at the current pointer location.
        let cursor_x = state.pointer_location.x.round() as i32;
        let cursor_y = state.pointer_location.y.round() as i32;
        let cursor_geo: Rectangle<i32, Physical> = Rectangle::new(
            (cursor_x, cursor_y).into(),
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

        let mut visible_surfaces: HashSet<wayland_server::Weak<WlSurface>> = HashSet::new();
        let mut frame_roots: Vec<WlSurface> = Vec::new();

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

            frame_roots.push(surface.clone());

            with_surface_tree_downward(
                &surface,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |child_surface, child_states, _| {
                    let data = child_states
                        .data_map
                        .get::<RendererSurfaceStateUserData>();
                    let Some(data) = data else {
                        return;
                    };
                    if data.lock().unwrap().view().is_some() {
                        self.output.enter(child_surface);
                        visible_surfaces.insert(child_surface.downgrade());
                    }
                },
                |_, _, _| true,
            );

            let location: Point<i32, Physical> = (geo.x, geo.y).into();
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
        }

        // Send wl_surface.leave for surfaces no longer visible on this output.
        // (Best-effort; weak surfaces that are gone are just dropped.)
        for gone in self.surfaces_on_output.difference(&visible_surfaces) {
            if let Ok(surf) = gone.upgrade() {
                self.output.leave(&surf);
            }
        }
        self.surfaces_on_output = visible_surfaces.clone();

        // A visible, unmistakable background.
        let (w, h) = self.mode_size;
        let bg_geo = Rectangle::<i32, Physical>::from_size((w, h).into());
        let bg = SolidColorRenderElement::new(
            self.background_id.clone(),
            bg_geo,
            0usize,
            smithay::backend::renderer::Color32F::new(0.1, 0.15, 0.25, 1.0),
            Kind::Unspecified,
        );

        elements.push(KmsRenderElement::Solid(bg));

        // Render and queue a frame. We always queue so the scanout is updated.
        match self.drm_output.render_frame(
            &mut self.renderer,
            &elements,
            smithay::backend::renderer::Color32F::new(0.0, 0.0, 0.0, 1.0),
            FrameFlags::DEFAULT,
        ) {
            Ok(res) => {
                // If Smithay considers this frame empty, skip queueing; it will never pageflip.
                if res.is_empty {
                    self.needs_render = false;
                    self.send_frame_callbacks = false;
                    self.frame_callback_roots.clear();
                    self.frame_callback_visible.clear();
                    return;
                }

                if let Err(err) = self.drm_output.queue_frame(()) {
                    tracing::warn!("drm queue_frame failed: {err:?}");
                } else {
                    self.frame_pending = true;
                    self.send_frame_callbacks = true;
                    self.frame_callback_roots = frame_roots;
                    self.frame_callback_visible = visible_surfaces;
                }
            }
            Err(err) => {
                // This is usually recoverable (e.g. device inactive). Keep trying.
                tracing::warn!("drm render_frame failed: {err:?}");
            }
        }

        self.needs_render = false;
    }

    pub fn on_vblank(&mut self, crtc: crtc::Handle, _metadata: &mut Option<DrmEventMetadata>) {
        if crtc != self.crtc {
            return;
        }

        if let Err(err) = self.drm_output.frame_submitted() {
            // Common on VT switch / permission issues.
            tracing::debug!("drm frame_submitted error: {err:?}");
        }
        self.frame_pending = false;

        if self.send_frame_callbacks {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO);

            let throttle = self.frame_callback_throttle;
            let output = self.output.clone();
            let visible = self.frame_callback_visible.clone();

            for root in &self.frame_callback_roots {
                // Only surfaces we actually rendered should be considered on the primary scanout.
                // This avoids sending frame callbacks for completely occluded surfaces.
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

            self.send_frame_callbacks = false;
            self.frame_callback_roots.clear();
            self.frame_callback_visible.clear();
        }
    }
}

fn pick_crtc(
    drm_device: &DrmDevice,
    res: &smithay::reexports::drm::control::ResourceHandles,
    conn: &connector::Info,
) -> Option<crtc::Handle> {
    // Prefer encoder's current CRTC, otherwise pick the first possible.
    for enc_handle in conn.encoders() {
        let enc = drm_device.get_encoder(*enc_handle).ok()?;
        if let Some(crtc) = enc.crtc() {
            return Some(crtc);
        }

        let possible = enc.possible_crtcs();
        if let Some(crtc) = res.filter_crtcs(possible).into_iter().next() {
            return Some(crtc);
        }
    }

    None
}
