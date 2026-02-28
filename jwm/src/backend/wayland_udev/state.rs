use crate::backend::api::{BackendEvent, Geometry, LayerSurfaceInfo, PropertyKind};
use crate::backend::common_define::WindowId;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use log::{debug, info, warn};

use smithay::delegate_compositor;
use smithay::delegate_data_device;
use smithay::delegate_layer_shell;
use smithay::delegate_output;
use smithay::delegate_primary_selection;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_viewporter;
use smithay::delegate_xdg_shell;
use smithay::delegate_text_input_manager;
use smithay::delegate_input_method_manager;
use smithay::delegate_virtual_keyboard_manager;
use smithay::delegate_xdg_activation;
use smithay::delegate_xwayland_shell;
use smithay::xwayland::{X11Wm, X11Surface, XwmHandler, XWaylandClientData, xwm::{Reorder, ResizeEdge as XwmResizeEdge, XwmId, WmWindowProperty}};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::input::keyboard::XkbConfig;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::protocol::wl_output::WlOutput;
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::{Logical, Point, Rectangle, Serial, SERIAL_COUNTER as SCOUNTER};
use smithay::desktop::{find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output, LayerSurface as DesktopLayerSurface, PopupKind, WindowSurfaceType};
use smithay::output::Output;
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::Format as DmabufFormat;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes};
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::shell::wlr_layer::{Anchor, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState};
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, SurfaceCachedState, ToplevelSurface, XdgShellHandler, XdgShellState};
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler};
use smithay::wayland::selection::primary_selection::{PrimarySelectionHandler, PrimarySelectionState};
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::text_input::TextInputManagerState;
use smithay::wayland::input_method::{InputMethodHandler, InputMethodManagerState, PopupSurface as ImPopupSurface};
use smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState;
use smithay::wayland::xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData};

#[derive(Debug, Default)]
pub struct JwmClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for JwmClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

pub struct JwmWaylandState {
    pub display_handle: DisplayHandle,
    pub pending_events: Arc<Mutex<std::collections::VecDeque<BackendEvent>>>,

    pub pointer_location: Point<f64, Logical>,
    pub needs_redraw: bool,

    pub output_manager_state: OutputManagerState,

    pub compositor_state: CompositorState,
    pub shm_state: ShmState,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub seat_state: SeatState<JwmWaylandState>,
    pub seat: Seat<JwmWaylandState>,
    pub xdg_shell_state: XdgShellState,
    pub viewporter_state: ViewporterState,

    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,

    pub layer_shell_state: WlrLayerShellState,

    pub xdg_activation_state: XdgActivationState,

    /// XWayland shell state (for associating X11 windows with wl_surfaces).
    pub xwayland_shell_state: XWaylandShellState,

    /// The X11 WM instance (set after XWayland becomes ready).
    pub x11_wm: Option<X11Wm>,

    /// Map from X11Surface window_id -> our WindowId.
    pub x11_surface_to_window: HashMap<u32, WindowId>,

    /// Map from our WindowId -> X11Surface (for property queries etc.).
    pub x11_surfaces: HashMap<WindowId, X11Surface>,

    /// XWayland may associate a `wl_surface` with an X11 window before we allocate a `WindowId`.
    /// Stash the association so we can wire it up once `map_window_request`/`mapped_override_redirect_window`
    /// allocates the window.
    pub pending_x11_wl_surfaces: HashMap<u32, WlSurface>,

    /// KMS-backed outputs currently available for mapping layer surfaces.
    pub outputs: Vec<Output>,

    pub next_window_raw: u64,
    pub toplevels: HashMap<WindowId, ToplevelSurface>,
    pub surface_to_window: HashMap<ObjectId, WindowId>,

    pub pending_initial_configure: HashMap<WindowId, Instant>,

    pub popups: HashMap<ObjectId, PopupSurface>,
    pub popup_order: Vec<ObjectId>,

    pub active_toplevel: Option<WindowId>,
    pub popup_grab_toplevel: Option<WindowId>,
    pub popup_grab_prev_kbd_focus: Option<WlSurface>,
    pub output_rects: Vec<Rectangle<i32, Logical>>,

    pub window_geometry: HashMap<WindowId, Geometry>,
    pub window_stack: Vec<WindowId>,

    pub mapped_windows: HashSet<WindowId>,
    pub window_title: HashMap<WindowId, String>,
    pub window_app_id: HashMap<WindowId, String>,
    pub window_is_fullscreen: HashMap<WindowId, bool>,

    pub window_layer_info: HashMap<WindowId, LayerSurfaceInfo>,

    /// Per-window border color (ARGB, used for server-side decoration in tiling WM).
    pub window_border_color: HashMap<WindowId, [f32; 4]>,

    /// Shared queue for pending wlr-screencopy copy requests (filled by screencopy Dispatch,
    /// drained during KMS render).
    pub screencopy_pending: Option<crate::backend::wayland_udev::screencopy::PendingScreencopyQueue>,
}

impl JwmWaylandState {
    fn surface_window_geometry_loc(&self, surface: &WlSurface) -> Point<i32, Logical> {
        // xdg_surface.set_window_geometry sets this. When non-zero, the compositor must shift the
        // wl_surface buffer origin by -loc so the window-geometry aligns with the WM's x/y.
        with_states(surface, |states| {
            let mut cached = states.cached_state.get::<SurfaceCachedState>();
            cached
                .current()
                .geometry
                .map(|r| r.loc)
                .unwrap_or_else(|| (0, 0).into())
        })
    }

    fn toplevel_buffer_origin(&self, win: WindowId) -> Option<Point<i32, Logical>> {
        let geo = self.window_geometry.get(&win).copied()?;
        let surface = self.surface_for_window(win)?;
        let offset = self.surface_window_geometry_loc(&surface);
        Some((geo.x - offset.x, geo.y - offset.y).into())
    }

    fn popup_buffer_origin(
        &self,
        win: WindowId,
        popup_surface: &WlSurface,
        popup_rect: Rectangle<i32, Logical>,
    ) -> Option<Point<i32, Logical>> {
        // `popup_rect.loc` is the window-geometry origin of the popup in global coords.
        // Convert it to the actual buffer origin by subtracting the committed geometry loc.
        let _ = win;
        let offset = self.surface_window_geometry_loc(popup_surface);
        Some((popup_rect.loc.x - offset.x, popup_rect.loc.y - offset.y).into())
    }

    pub fn ensure_dmabuf_global(
        &mut self,
        display_handle: &DisplayHandle,
        formats: impl IntoIterator<Item = DmabufFormat>,
    ) {
        if self.dmabuf_global.is_some() {
            return;
        }

        let global = self
            .dmabuf_state
            .create_global::<JwmWaylandState>(display_handle, formats);
        self.dmabuf_global = Some(global);
        info!("[udev/wayland] linux-dmabuf global created");
    }
}

delegate_compositor!(JwmWaylandState);

delegate_shm!(JwmWaylandState);

delegate_seat!(JwmWaylandState);

delegate_xdg_shell!(JwmWaylandState);

delegate_layer_shell!(JwmWaylandState);

delegate_output!(JwmWaylandState);

delegate_data_device!(JwmWaylandState);

delegate_primary_selection!(JwmWaylandState);

delegate_viewporter!(JwmWaylandState);

delegate_text_input_manager!(JwmWaylandState);

delegate_input_method_manager!(JwmWaylandState);

delegate_virtual_keyboard_manager!(JwmWaylandState);

delegate_xdg_activation!(JwmWaylandState);

delegate_xwayland_shell!(JwmWaylandState);

smithay::delegate_dmabuf!(JwmWaylandState);

// ---------------------------------------------------------------------------
// XDG Activation Handler – allows clients to request surface activation
// ---------------------------------------------------------------------------
impl XdgActivationHandler for JwmWaylandState {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        // Accept activations from tokens younger than 10 seconds.
        if token_data.timestamp.elapsed().as_secs() < 10 {
            // Find the window that corresponds to this surface and activate it.
            if let Some(&win_id) = self.surface_to_window.get(&surface.id()) {
                debug!("[xdg_activation] activating window {:?} (app_id={:?})", win_id, token_data.app_id);
                self.active_toplevel = Some(win_id);
                self.needs_redraw = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// XWayland Shell Handler – associates X11 windows with Wayland surfaces
// ---------------------------------------------------------------------------
impl XWaylandShellHandler for JwmWaylandState {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(
        &mut self,
        _xwm: XwmId,
        wl_surface: WlSurface,
        window: X11Surface,
    ) {
        let x11_id = window.window_id();
        debug!(
            "[xwayland] surface_associated: x11={} wl={:?} title={:?}",
            x11_id,
            wl_surface.id(),
            window.title(),
        );

        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            self.surface_to_window.insert(wl_surface.id(), win_id);
            self.needs_redraw = true;
        } else {
            self.pending_x11_wl_surfaces.insert(x11_id, wl_surface);
        }
    }
}

// ---------------------------------------------------------------------------
// XWM Handler – manages X11 windows running under XWayland
// ---------------------------------------------------------------------------
impl XwmHandler for JwmWaylandState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.x11_wm.as_mut().expect("X11Wm not yet started")
    }

    fn new_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!(
            "[xwayland] new_window: id={} title={:?} class={:?} override_redirect={}",
            window.window_id(),
            window.title(),
            window.class(),
            window.is_override_redirect(),
        );
    }

    fn new_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        debug!(
            "[xwayland] new_override_redirect_window: id={} class={:?}",
            window.window_id(),
            window.class(),
        );
    }

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        info!(
            "[xwayland] map_window_request: id={} title={:?} class={:?}",
            window.window_id(),
            window.title(),
            window.class(),
        );

        // Grant the map request.
        if let Err(e) = window.set_mapped(true) {
            warn!("[xwayland] set_mapped(true) failed: {e:?}");
            return;
        }

        // Send a configure with the requested geometry (or a reasonable default).
        let geo = window.geometry();
        let w = if geo.size.w > 0 { geo.size.w as u32 } else { 800 };
        let h = if geo.size.h > 0 { geo.size.h as u32 } else { 600 };
        let _ = window.configure(Some(smithay::utils::Rectangle::new(
            (geo.loc.x, geo.loc.y).into(),
            (w as i32, h as i32).into(),
        )));

        // Allocate a WindowId and track the surface.
        let win_id = self.alloc_window_id();
        let x11_id = window.window_id();
        self.x11_surface_to_window.insert(x11_id, win_id);
        self.x11_surfaces.insert(win_id, window.clone());

        if let Some(wl_surface) = self.pending_x11_wl_surfaces.remove(&x11_id) {
            self.surface_to_window.insert(wl_surface.id(), win_id);
        }
        self.window_geometry.insert(
            win_id,
            Geometry {
                x: geo.loc.x,
                y: geo.loc.y,
                w,
                h,
                border: 0,
            },
        );
        self.window_title
            .insert(win_id, window.title());
        self.window_app_id
            .insert(win_id, window.class());
        self.window_is_fullscreen
            .insert(win_id, window.is_fullscreen());
        self.window_stack.push(win_id);

        // X11 windows don't go through our Wayland-commit mapping path unless we link the associated
        // wl_surface. Mark them mapped here so they participate in rendering/hit-testing immediately.
        self.mapped_windows.insert(win_id);
        self.needs_redraw = true;

        self.push_event(BackendEvent::WindowCreated(win_id));
        self.push_event(BackendEvent::WindowMapped(win_id));
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        info!(
            "[xwayland] mapped_override_redirect: id={} class={:?}",
            window.window_id(),
            window.class(),
        );

        // Override-redirect windows (menus, tooltips, etc.) are managed separately.
        let win_id = self.alloc_window_id();
        let x11_id = window.window_id();
        self.x11_surface_to_window.insert(x11_id, win_id);
        self.x11_surfaces.insert(win_id, window.clone());

        if let Some(wl_surface) = self.pending_x11_wl_surfaces.remove(&x11_id) {
            self.surface_to_window.insert(wl_surface.id(), win_id);
        }

        let geo = window.geometry();
        self.window_geometry.insert(
            win_id,
            Geometry {
                x: geo.loc.x,
                y: geo.loc.y,
                w: geo.size.w.max(1) as u32,
                h: geo.size.h.max(1) as u32,
                border: 0,
            },
        );
        self.window_title
            .insert(win_id, window.title());
        self.window_app_id
            .insert(win_id, window.class());
        self.window_is_fullscreen.insert(win_id, false);
        self.window_stack.push(win_id);

        self.mapped_windows.insert(win_id);
        self.needs_redraw = true;

        self.push_event(BackendEvent::WindowCreated(win_id));
        self.push_event(BackendEvent::WindowMapped(win_id));
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let x11_id = window.window_id();
        info!("[xwayland] unmapped_window: id={}", x11_id);

        if let Some(win_id) = self.x11_surface_to_window.remove(&x11_id) {
            self.x11_surfaces.remove(&win_id);
            let was_mapped = self.mapped_windows.remove(&win_id);
            self.surface_to_window.retain(|_, w| *w != win_id);
            self.window_geometry.remove(&win_id);
            self.window_stack.retain(|w| *w != win_id);
            self.window_title.remove(&win_id);
            self.window_app_id.remove(&win_id);
            self.window_is_fullscreen.remove(&win_id);
            self.window_border_color.remove(&win_id);

            self.needs_redraw = true;

            if was_mapped {
                self.push_event(BackendEvent::WindowUnmapped(win_id));
            }
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let x11_id = window.window_id();
        info!("[xwayland] destroyed_window: id={}", x11_id);

        if let Some(win_id) = self.x11_surface_to_window.remove(&x11_id) {
            self.x11_surfaces.remove(&win_id);
            self.mapped_windows.remove(&win_id);
            self.surface_to_window.retain(|_, w| *w != win_id);
            self.window_geometry.remove(&win_id);
            self.window_stack.retain(|w| *w != win_id);
            self.window_title.remove(&win_id);
            self.window_app_id.remove(&win_id);
            self.window_is_fullscreen.remove(&win_id);
            self.window_border_color.remove(&win_id);

            self.needs_redraw = true;

            self.push_event(BackendEvent::WindowDestroyed(win_id));
        }
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let x11_id = window.window_id();
        debug!(
            "[xwayland] configure_request: id={} x={:?} y={:?} w={:?} h={:?}",
            x11_id, x, y, w, h
        );

        // Apply the requested geometry.
        let geo = window.geometry();
        let new_x = x.unwrap_or(geo.loc.x);
        let new_y = y.unwrap_or(geo.loc.y);
        let new_w = w.unwrap_or(geo.size.w.max(1) as u32);
        let new_h = h.unwrap_or(geo.size.h.max(1) as u32);

        let _ = window.configure(Some(smithay::utils::Rectangle::new(
            (new_x, new_y).into(),
            (new_w as i32, new_h as i32).into(),
        )));

        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            self.window_geometry.insert(
                win_id,
                Geometry {
                    x: new_x,
                    y: new_y,
                    w: new_w,
                    h: new_h,
                    border: 0,
                },
            );
            self.push_event(BackendEvent::WindowConfigured {
                window: win_id,
                x: new_x,
                y: new_y,
                width: new_w,
                height: new_h,
            });
        }

        self.needs_redraw = true;
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
        let x11_id = window.window_id();
        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            self.window_geometry.insert(
                win_id,
                Geometry {
                    x: geometry.loc.x,
                    y: geometry.loc.y,
                    w: geometry.size.w.max(1) as u32,
                    h: geometry.size.h.max(1) as u32,
                    border: 0,
                },
            );
        }
        self.needs_redraw = true;
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: XwmResizeEdge,
    ) {
        // Interactive resize not yet supported for X11 windows.
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {
        // Interactive move not yet supported for X11 windows.
    }

    fn property_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        property: WmWindowProperty,
    ) {
        let x11_id = window.window_id();
        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            match property {
                WmWindowProperty::Title => {
                    self.window_title.insert(win_id, window.title());
                    self.push_event(BackendEvent::PropertyChanged {
                        window: win_id,
                        kind: PropertyKind::Title,
                    });
                }
                WmWindowProperty::Class => {
                    self.window_app_id.insert(win_id, window.class());
                    self.push_event(BackendEvent::PropertyChanged {
                        window: win_id,
                        kind: PropertyKind::Class,
                    });
                }
                _ => {}
            }
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let x11_id = window.window_id();
        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            self.window_is_fullscreen.insert(win_id, true);
            let _ = window.set_fullscreen(true);
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let x11_id = window.window_id();
        if let Some(win_id) = self.x11_surface_to_window.get(&x11_id).copied() {
            self.window_is_fullscreen.insert(win_id, false);
            let _ = window.set_fullscreen(false);
        }
    }
}

impl JwmWaylandState {
    pub fn set_active_toplevel(&mut self, win: Option<WindowId>) {
        if self.active_toplevel == win {
            return;
        }

        let debug_focus = std::env::var("JWM_DEBUG_FOCUS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let prev = self.active_toplevel.take();
        if debug_focus {
            info!("[udev/focus] active_toplevel {:?} -> {:?}", prev, win);
        }

        if let Some(prev_win) = prev {
            if let Some(toplevel) = self.toplevels.get(&prev_win).cloned() {
                toplevel.with_pending_state(|s| {
                    s.states.unset(xdg_toplevel::State::Activated);
                });
                toplevel.send_configure();
            }
        }

        self.active_toplevel = win;
        if let Some(new_win) = win {
            if let Some(toplevel) = self.toplevels.get(&new_win).cloned() {
                toplevel.with_pending_state(|s| {
                    s.states.set(xdg_toplevel::State::Activated);
                });
                toplevel.send_configure();
            }
        }
    }

    pub fn init(
        dh: &DisplayHandle,
        handle: smithay::reexports::calloop::LoopHandle<'static, JwmWaylandState>,
        pending_events: Arc<Mutex<std::collections::VecDeque<BackendEvent>>>,
        seat_name: String,
        listen_on_socket: bool,
    ) -> Result<(Self, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
        let socket_name = if listen_on_socket {
            let source = ListeningSocketSource::new_auto()?;
            let socket_name = source.socket_name().to_string_lossy().into_owned();
            handle.insert_source(source, |client_stream, _, data| {
                match data
                    .display_handle
                    .insert_client(client_stream, Arc::new(JwmClientState::default()))
                {
                    Ok(client_id) => {
                        info!("[udev/wayland] client connected: {client_id:?}");
                    }
                    Err(e) => {
                        warn!("[udev/wayland] insert_client failed: {e:?}");
                    }
                }
            })?;
            Some(socket_name)
        } else {
            None
        };

        let compositor_state = CompositorState::new::<JwmWaylandState>(dh);
        let shm_state = ShmState::new::<JwmWaylandState>(
            dh,
            vec![wl_shm::Format::Argb8888, wl_shm::Format::Xrgb8888],
        );

        // Toolkits like GTK expect wl_data_device_manager (clipboard/DnD) and often primary
        // selection to be available.
        let data_device_state = DataDeviceState::new::<JwmWaylandState>(dh);
        let primary_selection_state = PrimarySelectionState::new::<JwmWaylandState>(dh);
        let xdg_shell_state = XdgShellState::new::<JwmWaylandState>(dh);
        let viewporter_state = ViewporterState::new::<JwmWaylandState>(dh);

        let dmabuf_state = DmabufState::new();

        let layer_shell_state = WlrLayerShellState::new::<JwmWaylandState>(dh);
        let xdg_activation_state = XdgActivationState::new::<JwmWaylandState>(dh);

        let xwayland_shell_state = XWaylandShellState::new::<JwmWaylandState>(dh);

        // wlr-screencopy-unstable-v1 – allows grim and similar tools to capture screen content.
        let screencopy_pending = crate::backend::wayland_udev::screencopy::init_screencopy_manager(dh);

        // Optional but very useful for toolkit compatibility.
        let output_manager_state = OutputManagerState::new_with_xdg_output::<JwmWaylandState>(dh);

        // IME / text input support – required for Chinese / Japanese / Korean input.
        TextInputManagerState::new::<JwmWaylandState>(dh);
        InputMethodManagerState::new::<JwmWaylandState, _>(dh, |_client| true);
        VirtualKeyboardManagerState::new::<JwmWaylandState, _>(dh, |_client| true);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, seat_name);
        seat.add_pointer();
        seat.add_keyboard(XkbConfig::default(), 200, 25)?;

        Ok((
            Self {
                display_handle: dh.clone(),
                pending_events,

                pointer_location: (0.0, 0.0).into(),
                needs_redraw: true,

                output_manager_state,
                compositor_state,
                shm_state,
                data_device_state,
                primary_selection_state,
                seat_state,
                seat,
                xdg_shell_state,
                viewporter_state,

                dmabuf_state,
                dmabuf_global: None,

                layer_shell_state,
                xdg_activation_state,
                xwayland_shell_state,
                x11_wm: None,
                x11_surface_to_window: HashMap::new(),
                x11_surfaces: HashMap::new(),
                pending_x11_wl_surfaces: HashMap::new(),
                active_toplevel: None,

                outputs: Vec::new(),
                next_window_raw: 1,
                toplevels: HashMap::new(),
                surface_to_window: HashMap::new(),

                pending_initial_configure: HashMap::new(),

                popups: HashMap::new(),
                popup_order: Vec::new(),

                popup_grab_toplevel: None,
                popup_grab_prev_kbd_focus: None,

                output_rects: Vec::new(),

                window_geometry: HashMap::new(),
                window_stack: Vec::new(),

                mapped_windows: HashSet::new(),
                window_title: HashMap::new(),
                window_app_id: HashMap::new(),
                window_is_fullscreen: HashMap::new(),

                window_layer_info: HashMap::new(),

                window_border_color: HashMap::new(),

                screencopy_pending: Some(screencopy_pending),
            },
            socket_name,
        ))
    }
    pub fn ensure_initial_configure_timeout(&mut self, timeout: Duration) {
        if self.pending_initial_configure.is_empty() {
            return;
        }

        let now = Instant::now();
        let expired: Vec<WindowId> = self
            .pending_initial_configure
            .iter()
            .filter_map(|(win, since)| {
                if now.duration_since(*since) >= timeout {
                    Some(*win)
                } else {
                    None
                }
            })
            .collect();

        for win in expired {
            // Only send a configure if the WM hasn't already done so.
            let Some(toplevel) = self.toplevels.get(&win).cloned() else {
                self.pending_initial_configure.remove(&win);
                continue;
            };

            if !toplevel.is_initial_configure_sent() {
                let (w, h) = self
                    .window_geometry
                    .get(&win)
                    .map(|g| (g.w, g.h))
                    .unwrap_or((800, 600));

                toplevel.with_pending_state(|s| {
                    s.size = Some((w as i32, h as i32).into());
                });
                let _ = toplevel.send_configure();
                self.needs_redraw = true;
            }

            self.pending_initial_configure.remove(&win);
        }
    }

    pub fn surface_under(
        &self,
        location: Point<f64, Logical>,
    ) -> Option<(Option<WindowId>, WlSurface, Point<f64, Logical>)> {
        // Layer surfaces should receive input before normal windows.
        for output in &self.outputs {
            let Some(mode) = output.current_mode() else {
                continue;
            };
            let scale = output.current_scale().fractional_scale();
            let logical_size = mode
                .size
                .to_f64()
                .to_logical(scale)
                .to_i32_round();
            let logical_size = output.current_transform().transform_size(logical_size);
            let rect = Rectangle::<i32, Logical>::new(output.current_location(), logical_size);
            if !rect.to_f64().contains(location) {
                continue;
            }

            let map = layer_map_for_output(output);

            // Prefer overlay then top layer for hit-testing.
            for layer in [Layer::Overlay, Layer::Top] {
                if let Some(ls) = map.layer_under(layer, location) {
                    if let Some(geo) = map.layer_geometry(ls) {
                        let origin: Point<f64, Logical> = (geo.loc.x as f64, geo.loc.y as f64).into();
                        return Some((None, ls.wl_surface().clone(), origin));
                    }
                }
            }
        }

        // Popups are always above their parent toplevel. Prefer them for hit-testing.
        for win in self.window_stack.iter().rev() {
            if !self.mapped_windows.contains(win) {
                continue;
            }

            for (popup_surface, popup_rect) in self.popup_rects_for_toplevel(*win) {
                let x0 = popup_rect.loc.x as f64;
                let y0 = popup_rect.loc.y as f64;
                let x1 = x0 + popup_rect.size.w as f64;
                let y1 = y0 + popup_rect.size.h as f64;
                if location.x >= x0 && location.y >= y0 && location.x < x1 && location.y < y1 {
                    let origin = self
                        .popup_buffer_origin(*win, &popup_surface, popup_rect)
                        .unwrap_or(popup_rect.loc);
                    return Some((
                        Some(*win),
                        popup_surface,
                        (origin.x as f64, origin.y as f64).into(),
                    ));
                }
            }
        }

        for win in self.window_stack.iter().rev() {
            if !self.mapped_windows.contains(win) {
                continue;
            }
            let geo = self.window_geometry.get(win)?;
            // Hit test includes border area so clicks on the border count as
            // clicking the window. `geo` stores the content-area origin
            // (x = original_x + bw), so expand outward by `border`.
            let bw = geo.border as f64;
            let x0 = geo.x as f64 - bw;
            let y0 = geo.y as f64 - bw;
            let x1 = geo.x as f64 + geo.w as f64 + bw;
            let y1 = geo.y as f64 + geo.h as f64 + bw;
            if location.x >= x0 && location.y >= y0 && location.x < x1 && location.y < y1 {
                if let Some(surface) = self.surface_for_window(*win) {
                    let origin = self.toplevel_buffer_origin(*win).unwrap_or((geo.x, geo.y).into());
                    return Some((
                        Some(*win),
                        surface,
                        (origin.x as f64, origin.y as f64).into(),
                    ));
                }
            }
        }

        None
    }

    fn popup_committed_geometry(popup: &PopupSurface) -> Option<Rectangle<i32, Logical>> {
        popup.with_committed_state(|s| s.map(|st| st.geometry))
    }

    fn popup_root_toplevel(&self, popup: &PopupSurface, depth: u8) -> Option<WindowId> {
        if depth > 16 {
            return None;
        }
        let parent = popup.get_parent_surface()?;
        let parent_id = parent.id();

        if let Some(win) = self.surface_to_window.get(&parent_id).copied() {
            return Some(win);
        }

        let parent_popup = self.popups.get(&parent_id)?;
        self.popup_root_toplevel(parent_popup, depth.saturating_add(1))
    }

    fn popup_global_origin(&self, popup: &PopupSurface, depth: u8) -> Option<Point<i32, Logical>> {
        if depth > 16 {
            return None;
        }
        let geo = Self::popup_committed_geometry(popup)?;

        let parent = popup.get_parent_surface()?;
        let parent_id = parent.id();

        if let Some(win) = self.surface_to_window.get(&parent_id).copied() {
            let parent_geo = self.window_geometry.get(&win)?;
            return Some((parent_geo.x + geo.loc.x, parent_geo.y + geo.loc.y).into());
        }

        let parent_popup = self.popups.get(&parent_id)?;
        let parent_origin = self.popup_global_origin(parent_popup, depth.saturating_add(1))?;
        Some((parent_origin.x + geo.loc.x, parent_origin.y + geo.loc.y).into())
    }

    pub fn dismiss_popups_for_toplevel(&mut self, win: WindowId) {
        // Send popup_done for all popups that belong to this toplevel grab.
        // Clients will unmap/destroy them asynchronously.
        let ids: Vec<ObjectId> = self
            .popup_order
            .iter()
            .filter(|id| {
                self.popups
                    .get(*id)
                    .is_some_and(|p| self.popup_root_toplevel(p, 0) == Some(win))
            })
            .cloned()
            .collect();

        for id in ids {
            if let Some(popup) = self.popups.get(&id) {
                popup.send_popup_done();
            }
        }
    }

    fn unconstrain_popup(&mut self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(win) = self.surface_to_window.get(&root.id()).copied() else {
            return;
        };

        let Some(window_geo) = self.window_geometry.get(&win).copied() else {
            return;
        };
        let window_rect: Rectangle<i32, Logical> = Rectangle::new(
            (window_geo.x, window_geo.y).into(),
            (window_geo.w as i32, window_geo.h as i32).into(),
        );

        let Some(mut outputs_geo) = self.output_rects.first().copied() else {
            return;
        };

        // Prefer constraining to the output that contains the parent toplevel (or pointer).
        // Falling back to the union of all outputs keeps behavior reasonable even if we can't
        // determine a best output.
        let best_output = {
            let window_center: Point<i32, Logical> = (
                window_geo.x + (window_geo.w as i32 / 2),
                window_geo.y + (window_geo.h as i32 / 2),
            )
                .into();

            let pointer: Point<i32, Logical> = (
                self.pointer_location.x.round() as i32,
                self.pointer_location.y.round() as i32,
            )
                .into();

            fn contains(rect: &Rectangle<i32, Logical>, p: Point<i32, Logical>) -> bool {
                p.x >= rect.loc.x
                    && p.y >= rect.loc.y
                    && p.x < rect.loc.x + rect.size.w
                    && p.y < rect.loc.y + rect.size.h
            }

            fn overlap_area(a: Rectangle<i32, Logical>, b: Rectangle<i32, Logical>) -> i64 {
                let x0 = a.loc.x.max(b.loc.x);
                let y0 = a.loc.y.max(b.loc.y);
                let x1 = (a.loc.x + a.size.w).min(b.loc.x + b.size.w);
                let y1 = (a.loc.y + a.size.h).min(b.loc.y + b.size.h);
                let w = (x1 - x0).max(0) as i64;
                let h = (y1 - y0).max(0) as i64;
                w * h
            }

            // 1) contains window center
            self.output_rects
                .iter()
                .find(|r| contains(r, window_center))
                .copied()
                // 2) contains pointer
                .or_else(|| self.output_rects.iter().find(|r| contains(r, pointer)).copied())
                // 3) max overlap with parent window rect
                .or_else(|| self.output_rects.iter().copied().max_by_key(|r| overlap_area(*r, window_rect)))
        };

        if let Some(rect) = best_output {
            outputs_geo = rect;
        } else {
            for rect in self.output_rects.iter().skip(1) {
                outputs_geo = outputs_geo.merge(*rect);
            }
        }

        // Target geometry for positioner is relative to the parent's window geometry.
        let mut target = outputs_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_rect.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    pub fn reconstrain_popups_for_toplevel(&mut self, win: WindowId) {
        if self.popups.is_empty() {
            return;
        }

        let popups: Vec<PopupSurface> = self
            .popup_order
            .iter()
            .filter_map(|id| {
                let popup = self.popups.get(id)?.clone();
                (self.popup_root_toplevel(&popup, 0) == Some(win)).then_some(popup)
            })
            .collect();

        for popup in popups {
            self.unconstrain_popup(&popup);
            let _ = popup.send_pending_configure();
        }

        self.needs_redraw = true;
    }

    pub fn popup_rects_for_toplevel(&self, win: WindowId) -> Vec<(WlSurface, Rectangle<i32, Logical>)> {
        // Front-to-back order: newest popups first.
        let mut out = Vec::new();

        for id in self.popup_order.iter().rev() {
            let Some(popup) = self.popups.get(id) else {
                continue;
            };
            if self.popup_root_toplevel(popup, 0) != Some(win) {
                continue;
            }

            let Some(geo) = Self::popup_committed_geometry(popup) else {
                continue;
            };
            let Some(origin) = self.popup_global_origin(popup, 0) else {
                continue;
            };

            let rect = Rectangle::<i32, Logical>::new(origin, geo.size);
            out.push((popup.wl_surface().clone(), rect));
        }

        out
    }

    pub fn popup_grab_area(&self, win: WindowId) -> Option<Rectangle<i32, Logical>> {
        // Define a conservative grab area: union of parent toplevel and all its popups.
        // This approximates the "popup grab" region well enough for toolkits.
        let parent_geo = self.window_geometry.get(&win).copied()?;
        let mut area: Rectangle<i32, Logical> = Rectangle::new(
            (parent_geo.x, parent_geo.y).into(),
            (parent_geo.w as i32, parent_geo.h as i32).into(),
        );

        for (_surf, rect) in self.popup_rects_for_toplevel(win) {
            area = area.merge(rect);
        }

        Some(area)
    }

    fn alloc_window_id(&mut self) -> WindowId {
        let id = WindowId::from_raw(self.next_window_raw);
        self.next_window_raw = self.next_window_raw.wrapping_add(1);
        id
    }

    fn push_event(&mut self, ev: BackendEvent) {
        self.pending_events.lock().unwrap().push_back(ev);
    }

    pub fn try_lookup_toplevel(&mut self, win: WindowId) -> Option<&mut ToplevelSurface> {
        self.toplevels.get_mut(&win)
    }

    pub fn surface_for_window(&self, win: WindowId) -> Option<WlSurface> {
        // Try Wayland toplevel first.
        if let Some(t) = self.toplevels.get(&win) {
            return Some(t.wl_surface().clone());
        }
        // Fall back to X11 surface.
        if let Some(x11) = self.x11_surfaces.get(&win) {
            return x11.wl_surface();
        }
        None
    }

    pub fn hit_test(&self, location: Point<f64, Logical>) -> Option<(WindowId, WlSurface, Point<f64, Logical>)> {
        self.surface_under(location)
            .and_then(|(win, surface, origin)| win.map(|w| (w, surface, origin)))
    }
}

impl OutputHandler for JwmWaylandState {}

impl CompositorHandler for JwmWaylandState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Regular Wayland clients are inserted with `JwmClientState`.
        if let Some(data) = client.get_data::<JwmClientState>() {
            return &data.compositor_state;
        }

        // XWayland is itself a Wayland client with `XWaylandClientData`.
        // Without this branch we would panic as soon as XWayland commits a surface.
        if let Some(data) = client.get_data::<XWaylandClientData>() {
            return &data.compositor_state;
        }

        panic!("Missing compositor client state (neither JwmClientState nor XWaylandClientData)")
    }

    fn commit(&mut self, surface: &WlSurface) {
        // Snapshot the buffer assignment kind BEFORE on_commit_buffer_handler consumes it.
        // on_commit_buffer_handler calls RendererSurfaceState::update_buffer which takes
        // the buffer out of SurfaceAttributes::current().buffer via .take(). If we read
        // the buffer afterwards it will always be None and windows will never be mapped.
        #[derive(Debug, Clone, Copy)]
        enum BufferState {
            NewBuffer,
            Removed,
            None,
        }

        let (buf_state, has_damage, has_buffer_delta) = with_states(surface, |states| {
            let mut cached = states.cached_state.get::<SurfaceAttributes>();
            let current = cached.current();
            let has_damage = !current.damage.is_empty();
            let has_buffer_delta = current.buffer_delta.is_some();
            let buf_state = match &current.buffer {
                Some(BufferAssignment::NewBuffer(_)) => BufferState::NewBuffer,
                Some(BufferAssignment::Removed) => BufferState::Removed,
                None => BufferState::None,
            };
            (buf_state, has_damage, has_buffer_delta)
        });

        // Keep renderer surface state in sync with wl_surface buffer commits.
        // Without this, WaylandSurfaceRenderElement will often have no view/texture and nothing
        // will be drawn even though windows are managed and receive input.
        on_commit_buffer_handler::<JwmWaylandState>(surface);

        let win = self.surface_to_window.get(&surface.id()).copied();

        // Root-surface mapping/unmapping -> translate into JWM window events.
        if let Some(win) = win {
            match buf_state {
                BufferState::NewBuffer => {
                    if self.mapped_windows.insert(win) {
                        info!("[udev/wayland] window mapped win={win:?}");

                        let offset = self.surface_window_geometry_loc(surface);
                        if offset.x != 0 || offset.y != 0 {
                            let geo = self.window_geometry.get(&win).copied();
                            debug!(
                                "[udev/wayland] mapped window-geometry offset win={win:?} surface_id={:?} window_geo={geo:?} xdg_loc=({}, {})",
                                surface.id(),
                                offset.x,
                                offset.y
                            );
                        }

                        self.push_event(BackendEvent::WindowMapped(win));
                    }
                    self.needs_redraw = true;
                }
                BufferState::Removed => {
                    if self.mapped_windows.remove(&win) {
                        info!("[udev/wayland] window unmapped win={win:?}");
                        self.push_event(BackendEvent::WindowUnmapped(win));
                    }
                    self.needs_redraw = true;
                }
                BufferState::None => {}
            }
        }

        // Rendering changes without a buffer attach (damage, buffer offset, etc).
        if !matches!(buf_state, BufferState::None) || has_damage || has_buffer_delta {
            self.needs_redraw = true;
        }

        // Ensure initial configure for layer surfaces, similar to Anvil.
        // Layer surfaces cannot attach a buffer before the initial configure is acked.
        for output in &self.outputs {
            let mut map = layer_map_for_output(output);
            if map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_none()
            {
                continue;
            }

            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });

            map.arrange();

            if !initial_configure_sent {
                if let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
                    layer.layer_surface().send_configure();
                    self.needs_redraw = true;
                }
            }

            // Update tracked geometry for JWM and emit a configure notify when it changes.
            if let (Some(win), Some(layer)) = (win, map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)) {
                let layer_info = layer.layer_surface().with_cached_state(|data| LayerSurfaceInfo {
                    exclusive_zone: data.exclusive_zone.into(),
                    anchor_top: data.anchor.contains(Anchor::TOP),
                    anchor_bottom: data.anchor.contains(Anchor::BOTTOM),
                    anchor_left: data.anchor.contains(Anchor::LEFT),
                    anchor_right: data.anchor.contains(Anchor::RIGHT),
                });
                self.window_layer_info.insert(win, layer_info);

                if let Some(geo) = map.layer_geometry(layer) {
                    let new_geo = Geometry {
                        x: geo.loc.x,
                        y: geo.loc.y,
                        w: geo.size.w.max(0) as u32,
                        h: geo.size.h.max(0) as u32,
                        border: 0,
                    };

                    let changed = self
                        .window_geometry
                        .get(&win)
                        .map(|old| old.x != new_geo.x || old.y != new_geo.y || old.w != new_geo.w || old.h != new_geo.h)
                        .unwrap_or(true);

                    if changed {
                        self.window_geometry.insert(win, new_geo);
                        self.pending_events
                            .lock()
                            .unwrap()
                            .push_back(BackendEvent::WindowConfigured {
                                window: win,
                                x: new_geo.x,
                                y: new_geo.y,
                                width: new_geo.w,
                                height: new_geo.h,
                            });
                    }
                }
            }

            break;
        }
    }

    fn destroyed(&mut self, surface: &WlSurface) {
        // Cleanup any tracked popups as well.
        if self.popups.remove(&surface.id()).is_some() {
            self.popup_order.retain(|id| *id != surface.id());
            self.needs_redraw = true;
        }

        if let Some(win) = self.surface_to_window.remove(&surface.id()) {
            // If this surface is a layer-shell surface, ensure it is also removed from the layer map.
            for output in &self.outputs {
                let map = layer_map_for_output(output);
                let layer = map
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .cloned();
                drop(map);

                if let Some(layer) = layer {
                    let mut map = layer_map_for_output(output);
                    map.unmap_layer(&layer);
                    break;
                }
            }

            self.toplevels.remove(&win);
            self.pending_initial_configure.remove(&win);
            self.window_geometry.remove(&win);
            self.window_stack.retain(|w| *w != win);
            self.mapped_windows.remove(&win);
            self.window_title.remove(&win);
            self.window_app_id.remove(&win);
            self.window_is_fullscreen.remove(&win);
            self.window_layer_info.remove(&win);
            self.window_border_color.remove(&win);
            self.push_event(BackendEvent::WindowDestroyed(win));
            self.needs_redraw = true;
        }
    }
}

impl ShmHandler for JwmWaylandState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl BufferHandler for JwmWaylandState {
    fn buffer_destroyed(&mut self, _buffer: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer) {
    }
}

impl DmabufHandler for JwmWaylandState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        _dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Create the wl_buffer resource for the client. The actual renderer import happens
        // later when rendering the surface (via RendererSurfaceState).
        let _ = notifier.successful::<JwmWaylandState>();
        self.needs_redraw = true;
    }
}

impl SeatHandler for JwmWaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

impl InputMethodHandler for JwmWaylandState {
    fn new_popup(&mut self, _surface: ImPopupSurface) {
        // IME popup surfaces (candidate window) – currently not rendered by JWM.
    }

    fn dismiss_popup(&mut self, _surface: ImPopupSurface) {}

    fn popup_repositioned(&mut self, _surface: ImPopupSurface) {}

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        // Return the geometry of the toplevel that owns this surface so the IME
        // popup can position itself correctly.
        if let Some(win) = self.surface_to_window.get(&parent.id()).copied() {
            if let Some(geo) = self.window_geometry.get(&win) {
                return Rectangle::new((geo.x, geo.y).into(), (geo.w as i32, geo.h as i32).into());
            }
        }
        Rectangle::default()
    }
}

impl SelectionHandler for JwmWaylandState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for JwmWaylandState {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl ClientDndGrabHandler for JwmWaylandState {}

impl ServerDndGrabHandler for JwmWaylandState {}

impl PrimarySelectionHandler for JwmWaylandState {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.primary_selection_state
    }
}

impl XdgShellHandler for JwmWaylandState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let win = self.alloc_window_id();
        let obj_id = surface.wl_surface().id();

        info!("[udev/wayland] new_toplevel win={win:?} surface_id={obj_id:?}");

        self.surface_to_window.insert(obj_id, win);
        self.toplevels.insert(win, surface);

        self.window_geometry.insert(
            win,
            Geometry {
                x: 0,
                y: 0,
                // Placeholder size until the WM configures the window.
                // Clients will typically wait for the initial configure before attaching a buffer.
                w: 800,
                h: 600,
                border: 0,
            },
        );
        self.window_stack.push(win);

        self.window_title.insert(win, String::new());
        self.window_app_id.insert(win, String::new());
        self.window_is_fullscreen.insert(win, false);

        // Track windows that still need their initial configure. Normally the WM triggers this via
        // `WindowOps::configure`, but we keep a timeout-based fallback to avoid clients stalling
        // indefinitely if the WM doesn't configure quickly enough.
        self.pending_initial_configure.insert(win, Instant::now());

        self.push_event(BackendEvent::WindowCreated(win));
        self.needs_redraw = true;
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        // Store the initial positioner state and compute a constrained geometry.
        surface.with_pending_state(|state| {
            state.positioner = positioner;
            state.geometry = state.positioner.get_geometry();
        });
        self.unconstrain_popup(&surface);
        let _ = surface.send_configure();

        let id = surface.wl_surface().id();
        self.popup_order.push(id.clone());
        self.popups.insert(id, surface);
        self.needs_redraw = true;
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat, _serial: Serial) {
        // Record the toplevel this grab belongs to, and remember current keyboard focus.
        if self.popup_grab_prev_kbd_focus.is_none() {
            self.popup_grab_prev_kbd_focus = self
                .seat
                .get_keyboard()
                .and_then(|k| k.current_focus());
        }

        let toplevel = if let Some(existing) = self.popups.get(&_surface.wl_surface().id()) {
            self.popup_root_toplevel(existing, 0)
        } else {
            self.popup_root_toplevel(&_surface, 0)
        };
        self.popup_grab_toplevel = toplevel;

        // Give the popup keyboard focus (menus often need this), while we remember the previous focus
        // for restoration when the grab ends.
        if let Some(kbd) = self.seat.get_keyboard() {
            let serial = SCOUNTER.next_serial();
            kbd.set_focus(self, Some(_surface.wl_surface().clone()), serial);
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.positioner = positioner;
            state.geometry = state.positioner.get_geometry();
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(win) = self.surface_to_window.remove(&surface.wl_surface().id()) {
            info!("[udev/wayland] toplevel_destroyed win={win:?}");
            self.toplevels.remove(&win);
            self.pending_initial_configure.remove(&win);
            self.window_geometry.remove(&win);
            self.window_stack.retain(|w| *w != win);
            self.mapped_windows.remove(&win);
            self.window_title.remove(&win);
            self.window_app_id.remove(&win);
            self.window_is_fullscreen.remove(&win);
            self.window_border_color.remove(&win);
            self.push_event(BackendEvent::WindowDestroyed(win));
            self.needs_redraw = true;
        }
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        let id = surface.wl_surface().id();
        self.popups.remove(&id);
        self.popup_order.retain(|x| *x != id);
        self.needs_redraw = true;

        if let Some(grab_win) = self.popup_grab_toplevel {
            let any_left = self
                .popups
                .values()
                .any(|p| self.popup_root_toplevel(p, 0) == Some(grab_win));
            if !any_left {
                self.popup_grab_toplevel = None;

                // Restore keyboard focus to what it was before the popup grab.
                if let Some(kbd) = self.seat.get_keyboard() {
                    let serial = SCOUNTER.next_serial();
                    if let Some(prev) = self.popup_grab_prev_kbd_focus.take() {
                        kbd.set_focus(self, Some(prev), serial);
                    } else if let Some(surface) = self.surface_for_window(grab_win) {
                        kbd.set_focus(self, Some(surface), serial);
                    }
                }
            }
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        let Some(win) = self
            .surface_to_window
            .get(&surface.wl_surface().id())
            .copied()
        else {
            return;
        };

        let app_id = with_states(surface.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .app_id
                .clone()
                .unwrap_or_default()
        });

        info!("[udev/wayland] app_id_changed win={win:?} app_id={}", app_id);

        self.window_app_id.insert(win, app_id);
        self.push_event(BackendEvent::PropertyChanged {
            window: win,
            kind: PropertyKind::Class,
        });
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        let Some(win) = self
            .surface_to_window
            .get(&surface.wl_surface().id())
            .copied()
        else {
            return;
        };

        let title = with_states(surface.wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .title
                .clone()
                .unwrap_or_default()
        });

        info!("[udev/wayland] title_changed win={win:?} title={}", title);

        self.window_title.insert(win, title);
        self.push_event(BackendEvent::PropertyChanged {
            window: win,
            kind: PropertyKind::Title,
        });
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        let Some(win) = self
            .surface_to_window
            .get(&surface.wl_surface().id())
            .copied()
        else {
            return;
        };

        self.push_event(BackendEvent::PropertyChanged {
            window: win,
            kind: PropertyKind::TransientFor,
        });
    }
}

impl WlrLayerShellHandler for JwmWaylandState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        output: Option<WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| {
                // If the client didn't pick an output, prefer the one under the pointer.
                let location = self.pointer_location;
                self.outputs.iter().find_map(|o| {
                    let Some(mode) = o.current_mode() else {
                        return None;
                    };
                    let scale = o.current_scale().fractional_scale();
                    let logical_size = mode
                        .size
                        .to_f64()
                        .to_logical(scale)
                        .to_i32_round();
                    let logical_size = o.current_transform().transform_size(logical_size);
                    let rect = Rectangle::<i32, Logical>::new(o.current_location(), logical_size);
                    if rect.to_f64().contains(location) {
                        Some(o.clone())
                    } else {
                        None
                    }
                })
            })
            .or_else(|| self.outputs.first().cloned());
        let Some(output) = output else {
            return;
        };

        // Log the client-provided intent; very useful to confirm whether bars are using layer-shell
        // and which anchors/exclusive zone they request.
        surface.with_cached_state(|data| {
            log::info!(
                "[layer-shell] new_surface ns='{}' layer={:?} anchor={:?} excl_zone={:?} size={:?} margin={:?} kbd={:?}",
                namespace,
                data.layer,
                data.anchor,
                data.exclusive_zone,
                data.size,
                data.margin,
                data.keyboard_interactivity
            );
        });

        let win = self.alloc_window_id();
        let obj_id = surface.wl_surface().id();

        let layer_info = surface.with_cached_state(|data| LayerSurfaceInfo {
            exclusive_zone: data.exclusive_zone.into(),
            anchor_top: data.anchor.contains(Anchor::TOP),
            anchor_bottom: data.anchor.contains(Anchor::BOTTOM),
            anchor_left: data.anchor.contains(Anchor::LEFT),
            anchor_right: data.anchor.contains(Anchor::RIGHT),
        });

        // Track as a JWM window so status bars (and other docks) can be detected via title/app_id.
        self.surface_to_window.insert(obj_id, win);
        self.window_layer_info.insert(win, layer_info);

        // Placeholder geometry until the layer map arranges and we observe it in `commit()`.
        self.window_geometry.insert(
            win,
            Geometry {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
                border: 0,
            },
        );
        self.window_title.insert(win, namespace.clone());
        self.window_app_id.insert(win, namespace.clone());
        self.window_is_fullscreen.insert(win, false);

        self.push_event(BackendEvent::WindowCreated(win));

        let mut map = layer_map_for_output(&output);
        let _ = map.map_layer(&DesktopLayerSurface::new(surface, namespace));
        self.needs_redraw = true;
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        for output in &self.outputs {
            let map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|&layer| layer.layer_surface() == &surface)
                .cloned();
            drop(map);

            if let Some(layer) = layer {
                let mut map = layer_map_for_output(output);
                map.unmap_layer(&layer);
                self.needs_redraw = true;
                break;
            }
        }
    }
}
