use crate::backend::api::{BackendEvent, Geometry, PropertyKind};
use crate::backend::common_define::WindowId;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use smithay::delegate_compositor;
use smithay::delegate_output;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::input::keyboard::XkbConfig;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::{Client, DisplayHandle, Resource};
use smithay::utils::{Logical, Point, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{with_states, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState};
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::output::OutputHandler;

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
    pub seat_state: SeatState<JwmWaylandState>,
    pub seat: Seat<JwmWaylandState>,
    pub xdg_shell_state: XdgShellState,

    pub next_window_raw: u64,
    pub toplevels: HashMap<WindowId, ToplevelSurface>,
    pub surface_to_window: HashMap<ObjectId, WindowId>,

    pub window_geometry: HashMap<WindowId, Geometry>,
    pub window_stack: Vec<WindowId>,

    pub mapped_windows: HashSet<WindowId>,
    pub window_title: HashMap<WindowId, String>,
    pub window_app_id: HashMap<WindowId, String>,
    pub window_is_fullscreen: HashMap<WindowId, bool>,
}

delegate_compositor!(JwmWaylandState);

delegate_shm!(JwmWaylandState);

delegate_seat!(JwmWaylandState);

delegate_xdg_shell!(JwmWaylandState);

delegate_output!(JwmWaylandState);

impl JwmWaylandState {
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
                let _ = data
                    .display_handle
                    .insert_client(client_stream, Arc::new(JwmClientState::default()));
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
        let xdg_shell_state = XdgShellState::new::<JwmWaylandState>(dh);

        // Optional but very useful for toolkit compatibility.
        let output_manager_state = OutputManagerState::new_with_xdg_output::<JwmWaylandState>(dh);

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
                seat_state,
                seat,
                xdg_shell_state,
                next_window_raw: 1,
                toplevels: HashMap::new(),
                surface_to_window: HashMap::new(),

                window_geometry: HashMap::new(),
                window_stack: Vec::new(),

                mapped_windows: HashSet::new(),
                window_title: HashMap::new(),
                window_app_id: HashMap::new(),
                window_is_fullscreen: HashMap::new(),
            },
            socket_name,
        ))
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
        self.toplevels.get(&win).map(|t| t.wl_surface().clone())
    }

    pub fn hit_test(&self, location: Point<f64, Logical>) -> Option<(WindowId, WlSurface, Point<f64, Logical>)> {
        for win in self.window_stack.iter().rev() {
            if !self.mapped_windows.contains(win) {
                continue;
            }
            let geo = self.window_geometry.get(win)?;
            let x0 = geo.x as f64;
            let y0 = geo.y as f64;
            let x1 = x0 + geo.w as f64;
            let y1 = y0 + geo.h as f64;
            if location.x >= x0 && location.y >= y0 && location.x < x1 && location.y < y1 {
                if let Some(surface) = self.surface_for_window(*win) {
                    return Some((*win, surface, (x0, y0).into()));
                }
            }
        }
        None
    }
}

impl OutputHandler for JwmWaylandState {}

impl CompositorHandler for JwmWaylandState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<JwmClientState>()
            .expect("Missing JwmClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        let win = self.surface_to_window.get(&surface.id()).copied();

        // Decide whether this commit impacts rendering.
        // Note: commits can be for subsurfaces too; we still want to redraw if they have damage/buffer.
        let (assignment, has_damage, has_buffer_delta) = with_states(surface, |states| {
            let mut cached = states.cached_state.get::<SurfaceAttributes>();
            let has_damage = !cached.current().damage.is_empty();
            let has_buffer_delta = cached.current().buffer_delta.is_some();
            let assignment = cached.current().buffer.take();
            (assignment, has_damage, has_buffer_delta)
        });

        // Root-surface mapping/unmapping -> translate into JWM window events.
        if let Some(win) = win {
            match assignment {
                Some(BufferAssignment::NewBuffer(_)) => {
                    if self.mapped_windows.insert(win) {
                        self.push_event(BackendEvent::WindowMapped(win));
                    }
                    self.needs_redraw = true;
                }
                Some(BufferAssignment::Removed) => {
                    if self.mapped_windows.remove(&win) {
                        self.push_event(BackendEvent::WindowUnmapped(win));
                    }
                    self.needs_redraw = true;
                }
                None => {}
            }
        }

        // Rendering changes without a buffer attach (damage, buffer offset, etc).
        if assignment.is_some() || has_damage || has_buffer_delta {
            self.needs_redraw = true;
        }
    }

    fn destroyed(&mut self, surface: &WlSurface) {
        if let Some(win) = self.surface_to_window.remove(&surface.id()) {
            self.toplevels.remove(&win);
            self.window_geometry.remove(&win);
            self.window_stack.retain(|w| *w != win);
            self.mapped_windows.remove(&win);
            self.window_title.remove(&win);
            self.window_app_id.remove(&win);
            self.window_is_fullscreen.remove(&win);
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

impl SeatHandler for JwmWaylandState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

impl XdgShellHandler for JwmWaylandState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let win = self.alloc_window_id();
        let obj_id = surface.wl_surface().id();

        surface.with_pending_state(|state| {
            state.size = Some((800, 600).into());
        });
        surface.send_configure();

        self.surface_to_window.insert(obj_id, win);
        self.toplevels.insert(win, surface);

        self.window_geometry.insert(
            win,
            Geometry {
                x: 0,
                y: 0,
                w: 800,
                h: 600,
                border: 0,
            },
        );
        self.window_stack.push(win);

        self.window_title.insert(win, String::new());
        self.window_app_id.insert(win, String::new());
        self.window_is_fullscreen.insert(win, false);

        self.push_event(BackendEvent::WindowCreated(win));
        self.needs_redraw = true;
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        // Minimal: just acknowledge with an initial configure.
        let _ = surface.send_configure();
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: smithay::reexports::wayland_server::protocol::wl_seat::WlSeat, _serial: Serial) {
        // Not implemented.
    }

    fn reposition_request(&mut self, surface: PopupSurface, _positioner: PositionerState, token: u32) {
        surface.send_repositioned(token);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some(win) = self.surface_to_window.remove(&surface.wl_surface().id()) {
            self.toplevels.remove(&win);
            self.window_geometry.remove(&win);
            self.window_stack.retain(|w| *w != win);
            self.mapped_windows.remove(&win);
            self.window_title.remove(&win);
            self.window_app_id.remove(&win);
            self.window_is_fullscreen.remove(&win);
            self.push_event(BackendEvent::WindowDestroyed(win));
            self.needs_redraw = true;
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

        self.window_title.insert(win, title);
        self.push_event(BackendEvent::PropertyChanged {
            window: win,
            kind: PropertyKind::Title,
        });
    }
}
