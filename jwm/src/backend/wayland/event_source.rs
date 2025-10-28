// src/backend/wayland/event_source.rs
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Receiver, Sender};

use super::window_ops::{WaylandRegistry, WindowRecord};
use crate::backend::api::{BackendEvent, EventSource, WindowId};

use smithay::reexports::wayland_server::Resource;
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{Seat, SeatHandler, SeatState},
    reexports::wayland_server::{
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_seat::WlSeat, wl_surface::WlSurface},
        Client, Display, DisplayHandle,
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        output::{OutputHandler, OutputManagerState},
        selection::data_device::{
            set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
            ServerDndGrabHandler,
        },
        selection::SelectionHandler,
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};

#[derive(Default)]
struct ClientState {
    compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

pub struct JwmWlState {
    display_handle: DisplayHandle,
    space: Arc<Mutex<Space<Window>>>,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    seat_state: SeatState<JwmWlState>,
    seat: Arc<Mutex<Seat<JwmWlState>>>,
    data_device_state: DataDeviceState,
    popups: PopupManager,

    tx: Sender<BackendEvent>,
    registry: Arc<Mutex<WaylandRegistry>>,
    seat_holder: Arc<Mutex<Option<Arc<Mutex<Seat<JwmWlState>>>>>>,
}

impl JwmWlState {
    fn new(
        dh: &DisplayHandle,
        tx: Sender<BackendEvent>,
        registry: Arc<Mutex<WaylandRegistry>>,
        space_arc: Arc<Mutex<Space<Window>>>,
        seat_holder: Arc<Mutex<Option<Arc<Mutex<Seat<JwmWlState>>>>>>,
    ) -> Self {
        let compositor_state = CompositorState::new::<JwmWlState>(dh);
        let xdg_shell_state = XdgShellState::new::<JwmWlState>(dh);
        let shm_state = ShmState::new::<JwmWlState>(dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<JwmWlState>(dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<JwmWlState>(dh);
        let popups = PopupManager::default();

        let mut seat: Seat<JwmWlState> = seat_state.new_wl_seat(dh, "jwm-wayland");
        let _ = seat.add_keyboard(Default::default(), 200, 25);
        seat.add_pointer();
        let seat_arc = Arc::new(Mutex::new(seat));
        {
            let mut h = seat_holder.lock().unwrap();
            *h = Some(seat_arc.clone());
        }

        Self {
            display_handle: dh.clone(),
            space: space_arc.clone(),

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            seat: seat_arc,
            data_device_state,
            popups,

            tx,
            registry,
            seat_holder,
        }
    }
}

impl CompositorHandler for JwmWlState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);

        if !is_sync_subsurface(surface) {
            let space_guard = self.space.lock().unwrap();
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = space_guard
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == &root)
            {
                window.on_commit();
            };
        }
    }
}

impl ShmHandler for JwmWlState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}
impl BufferHandler for JwmWlState {
    fn buffer_destroyed(
        &mut self,
        _buffer: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
    ) {
    }
}

impl SeatHandler for JwmWlState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<JwmWlState> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

impl OutputHandler for JwmWlState {}

impl DataDeviceHandler for JwmWlState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for JwmWlState {}
impl ServerDndGrabHandler for JwmWlState {}
impl SelectionHandler for JwmWlState {
    type SelectionUserData = ();
}

impl XdgShellHandler for JwmWlState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let mut space_guard = self.space.lock().unwrap();
        let window = Window::new_wayland_window(surface.clone());
        space_guard.map_element(window.clone(), (0, 0), false);

        surface.with_pending_state(|state| {
            state.size = Some(smithay::utils::Size::from((800, 600)));
        });
        surface.send_configure();

        let mut reg = self.registry.lock().unwrap();
        let new_id = (reg.windows.len() as u64) + 1;
        reg.windows.insert(
            new_id,
            WindowRecord {
                id: new_id,
                x: 0,
                y: 0,
                w: window.geometry().size.w,
                h: window.geometry().size.h,
                border: 0,
                handle: Some(window.clone()),
                wl_surface: Some(surface.wl_surface().clone()),
                toplevel: Some(surface),
            },
        );

        let _ = self.tx.send(BackendEvent::MapRequest {
            window: WindowId(new_id),
        });
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: smithay::utils::Serial) {}
    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }
}

delegate_compositor!(JwmWlState);
delegate_shm!(JwmWlState);
delegate_seat!(JwmWlState);
delegate_output!(JwmWlState);
delegate_xdg_shell!(JwmWlState);
delegate_data_device!(JwmWlState);

struct LoopData {
    state: JwmWlState,
    display_handle: DisplayHandle,
}

pub struct WaylandEventSource {
    rx: Receiver<BackendEvent>,
    tx: Sender<BackendEvent>,

    registry: Arc<Mutex<WaylandRegistry>>,

    space: Arc<Mutex<Space<Window>>>,
    seat_holder: Arc<Mutex<Option<Arc<Mutex<Seat<JwmWlState>>>>>>,
}

impl WaylandEventSource {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded();
        let registry = Arc::new(Mutex::new(WaylandRegistry::new()));

        let space_arc = Arc::new(Mutex::new(Space::default()));
        let seat_holder: Arc<Mutex<Option<Arc<Mutex<Seat<JwmWlState>>>>>> =
            Arc::new(Mutex::new(None));

        {
            let tx_clone = tx.clone();
            let reg_clone = registry.clone();
            let space_clone = space_arc.clone();
            let seat_holder_clone = seat_holder.clone();

            std::thread::spawn(move || {
                use smithay::reexports::calloop::{
                    generic::Generic, EventLoop, Interest, Mode, PostAction,
                };

                let mut event_loop: EventLoop<LoopData> = EventLoop::try_new().unwrap();
                let display: Display<JwmWlState> = Display::new().unwrap();
                let dh = display.handle();

                let state =
                    JwmWlState::new(&dh, tx_clone, reg_clone, space_clone, seat_holder_clone);
                let mut data = LoopData {
                    state,
                    display_handle: dh.clone(),
                };

                event_loop
                    .handle()
                    .insert_source(
                        ListeningSocketSource::new_auto().unwrap(),
                        move |client_stream, _, data| {
                            data.display_handle
                                .insert_client(client_stream, Arc::new(ClientState::default()))
                                .unwrap();
                        },
                    )
                    .expect("Failed to init Wayland event source");

                event_loop
                    .handle()
                    .insert_source(
                        Generic::new(display, Interest::READ, Mode::Level),
                        |_, display, data| {
                            unsafe {
                                let d = display.get_mut();
                                if let Err(e) = d.dispatch_clients(&mut data.state) {
                                    eprintln!("dispatch_clients error: {:?}", e);
                                }
                                if let Err(e) = d.flush_clients() {
                                    eprintln!("flush_clients error: {:?}", e);
                                }
                            }
                            Ok(PostAction::Continue)
                        },
                    )
                    .unwrap();

                event_loop.run(None, &mut data, move |_| {}).unwrap();
            });
        }

        Ok(Self {
            rx,
            tx,
            registry,
            space: space_arc,
            seat_holder,
        })
    }

    pub fn registry(&self) -> Arc<Mutex<WaylandRegistry>> {
        self.registry.clone()
    }
    pub fn space(&self) -> Arc<Mutex<Space<Window>>> {
        self.space.clone()
    }
    pub fn seat(&self) -> Arc<Mutex<Seat<JwmWlState>>> {
        loop {
            if let Some(seat_arc) = self.seat_holder.lock().unwrap().as_ref() {
                return seat_arc.clone();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl EventSource for WaylandEventSource {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        Ok(self.rx.try_recv().ok())
    }
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
