// src/backend/wayland/event_source.rs
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Receiver, Sender};

use super::window_ops::{WaylandRegistry, WindowRecord};
use crate::backend::api::{BackendEvent, EventSource, WindowId};

use smithay::wayland::output::OutputHandler;

use smithay::reexports::wayland_server::Resource;
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{Seat, SeatHandler, SeatState},
    output::{Mode, Output, PhysicalProperties, Subpixel},
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
        output::OutputManagerState,
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
    registry: Arc<Mutex<WaylandRegistry>>,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    data_device_state: DataDeviceState,
    popups: PopupManager,

    seat_state: SeatState<JwmWlState>,
    seat: Arc<Mutex<Seat<JwmWlState>>>,
    keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
    pointer: smithay::input::pointer::PointerHandle<JwmWlState>,

    tx: Sender<BackendEvent>,
}

impl JwmWlState {
    fn new(
        dh: &DisplayHandle,
        tx: Sender<BackendEvent>,
        registry: Arc<Mutex<WaylandRegistry>>,
        space_arc: Arc<Mutex<Space<Window>>>,
    ) -> Self {
        let compositor_state = CompositorState::new::<JwmWlState>(dh);
        let xdg_shell_state = XdgShellState::new::<JwmWlState>(dh);
        let shm_state = ShmState::new::<JwmWlState>(dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<JwmWlState>(dh);
        let data_device_state = DataDeviceState::new::<JwmWlState>(dh);
        let popups = PopupManager::default();

        let mut seat_state = SeatState::new();
        let mut seat: Seat<JwmWlState> = seat_state.new_wl_seat(dh, "jwm-wayland");
        let keyboard = seat
            .add_keyboard(Default::default(), 200, 25)
            .expect("add keyboard");
        let pointer = seat.add_pointer();
        let seat_arc = Arc::new(Mutex::new(seat));

        // 创建一个输出并映射到 space（参考 anvil winit）
        let output = Output::new(
            "JWM-Output".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Smithay".into(),
                model: "Internal".into(),
            },
        );
        let mode = Mode {
            size: (1280, 800).into(),
            refresh: 60_000,
        };
        output.create_global::<JwmWlState>(dh);
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
        output.set_preferred(mode);

        {
            let mut reg = registry.lock().unwrap();
            reg.screen_w = 1280;
            reg.screen_h = 800;
        }
        {
            let mut sp = space_arc.lock().unwrap();
            sp.map_output(&output, (0, 0));
        }

        Self {
            display_handle: dh.clone(),
            space: space_arc.clone(),
            registry: registry.clone(),

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            data_device_state,
            popups,

            seat_state,
            seat: seat_arc,
            keyboard,
            pointer,

            tx,
        }
    }

    fn window_id_from_surface(&self, s: &WlSurface) -> WindowId {
        let reg = self.registry.lock().unwrap();
        for (id, rec) in reg.windows.iter() {
            if let Some(ws) = rec.wl_surface.as_ref() {
                if ws == s {
                    return WindowId(*id);
                }
            }
        }
        WindowId(0)
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
        // 可在此缓存 cursor 状态（后续集成到 CursorProvider）
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
        space_guard.map_element(window.clone(), (50, 50), true);

        surface.with_pending_state(|pending| {
            pending.size = Some(smithay::utils::Size::from((800, 600)));
        });
        surface.send_configure();

        let mut reg = self.registry.lock().unwrap();
        let new_id = (reg.windows.len() as u64) + 1;
        reg.windows.insert(
            new_id,
            WindowRecord {
                id: new_id,
                x: 50,
                y: 50,
                w: window.geometry().size.w,
                h: window.geometry().size.h,
                border: 0,
                handle: Some(window.clone()),
                wl_surface: Some(surface.wl_surface().clone()),
                toplevel: Some(surface.clone()),
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
delegate_data_device!(JwmWlState); // 修复 GlobalDispatch<WlDataDeviceManager, ()> 约束

struct LoopData {
    state: JwmWlState,
    display_handle: DisplayHandle,
}

pub struct WaylandEventSource {
    rx: Receiver<BackendEvent>,
    tx: Sender<BackendEvent>,

    registry: Arc<Mutex<WaylandRegistry>>,

    space: Arc<Mutex<Space<Window>>>,
    seat: Arc<Mutex<Seat<JwmWlState>>>,
    pointer: smithay::input::pointer::PointerHandle<JwmWlState>,
    keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
}

impl WaylandEventSource {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded();
        let registry = Arc::new(Mutex::new(WaylandRegistry::new()));
        let space_arc = Arc::new(Mutex::new(Space::default()));

        let (seat_arc, pointer_handle, keyboard_handle) = {
            let tx_clone = tx.clone();
            let reg_clone = registry.clone();
            let space_clone = space_arc.clone();

            let (seat_sender, seat_receiver) = std::sync::mpsc::channel();

            std::thread::spawn(move || {
                use smithay::reexports::calloop::{
                    generic::Generic, EventLoop, Interest, Mode, PostAction,
                };

                let mut event_loop: EventLoop<LoopData> = EventLoop::try_new().unwrap();
                let display: Display<JwmWlState> = Display::new().unwrap();
                let dh = display.handle();

                let state = JwmWlState::new(&dh, tx_clone, reg_clone, space_clone);
                let mut data = LoopData {
                    state,
                    display_handle: dh.clone(),
                };

                let seat_arc = data.state.seat.clone();
                let pointer_handle = data.state.pointer.clone();
                let keyboard_handle = data.state.keyboard.clone();
                let _ = seat_sender.send((
                    seat_arc.clone(),
                    pointer_handle.clone(),
                    keyboard_handle.clone(),
                ));

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

            let (seat_arc, pointer, keyboard) =
                seat_receiver.recv().expect("Failed to get seat handles");
            (seat_arc, pointer, keyboard)
        };

        Ok(Self {
            rx,
            tx,
            registry,
            space: space_arc,
            seat: seat_arc,
            pointer: pointer_handle,
            keyboard: keyboard_handle,
        })
    }

    pub fn registry(&self) -> Arc<Mutex<WaylandRegistry>> {
        self.registry.clone()
    }
    pub fn space(&self) -> Arc<Mutex<Space<Window>>> {
        self.space.clone()
    }
    pub fn seat(&self) -> Arc<Mutex<Seat<JwmWlState>>> {
        self.seat.clone()
    }
    pub fn pointer_handle(&self) -> smithay::input::pointer::PointerHandle<JwmWlState> {
        self.pointer.clone()
    }
    pub fn keyboard_handle(&self) -> smithay::input::keyboard::KeyboardHandle<JwmWlState> {
        self.keyboard.clone()
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
