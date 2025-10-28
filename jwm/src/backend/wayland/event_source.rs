// src/backend/wayland/event_source.rs
use std::sync::{Arc, Mutex};

use calloop::channel::channel;
use smithay::reexports::calloop::{
    channel::Event as ChannelEvent, generic::Generic, EventLoop, Interest, Mode, PostAction,
};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::{
    backend::{ClientData, ClientId, DisconnectReason},
    Client, Display, DisplayHandle,
};
use smithay::wayland::output::OutputHandler;

use super::grabs::PointerMoveSurfaceGrab;
use super::window_ops::{WaylandRegistry, WindowRecord};
use crate::backend::api::{BackendEvent, EventSource, WindowId};
use crate::backend::common_define::Mods;
// smithay 0.7.0 specific imports
use smithay::backend::input::{Event, InputBackend, KeyState, KeyboardKeyEvent};
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::delegate_compositor;
use smithay::delegate_data_device;
use smithay::delegate_output;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::desktop::{PopupKind, PopupManager, Space, Window};
use smithay::input::keyboard::{FilterResult, KeysymHandle, ModifiersState}; // Import KeysymHandle
use smithay::input::pointer::Focus;
use smithay::input::{keyboard::KeyboardHandle, Seat, SeatHandler, SeatState}; // Simplified imports
use smithay::output::{Output, PhysicalProperties, Subpixel};
use smithay::reexports::wayland_server::protocol::{wl_seat::WlSeat, wl_surface::WlSurface};
use smithay::utils::{Serial, SERIAL_COUNTER}; // Serial is needed
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    get_parent, is_sync_subsurface, CompositorClientState, CompositorHandler, CompositorState,
};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
    ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::socket::ListeningSocketSource;

// 定义从 JWM Core 发往此线程的命令
#[derive(Debug)]
pub enum CompositorCommand {
    SetFocus(Option<WindowId>),
    StartMoveGrab(WindowId),
}

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
    pub space: Arc<Mutex<Space<Window>>>,
    pub registry: Arc<Mutex<WaylandRegistry>>,
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    data_device_state: DataDeviceState,
    popups: PopupManager,
    seat_state: SeatState<JwmWlState>,
    pub seat: Seat<JwmWlState>,
    pub keyboard: KeyboardHandle<JwmWlState>,
    pub pointer: smithay::input::pointer::PointerHandle<JwmWlState>,
    tx: smithay::reexports::calloop::channel::Sender<BackendEvent>,
}

impl JwmWlState {
    fn new(
        dh: &DisplayHandle,
        tx: smithay::reexports::calloop::channel::Sender<BackendEvent>,
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
        let keyboard = seat.add_keyboard(Default::default(), 200, 25).unwrap();
        let pointer = seat.add_pointer();

        let output = Output::new(
            "JWM-Output".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Smithay".into(),
                model: "Internal".into(),
            },
        );
        use smithay::output::Mode;
        let mode = Mode {
            size: (1280, 800).into(),
            refresh: 60_000,
        };
        output.create_global::<JwmWlState>(dh);
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
        output.set_preferred(mode);

        registry.lock().unwrap().screen_w = 1280;
        registry.lock().unwrap().screen_h = 800;
        space_arc.lock().unwrap().map_output(&output, (0, 0));

        Self {
            display_handle: dh.clone(),
            space: space_arc,
            registry,
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            data_device_state,
            popups,
            seat_state,
            seat,
            keyboard,
            pointer,
            tx,
        }
    }

    pub fn process_command(&mut self, cmd: CompositorCommand) {
        match cmd {
            CompositorCommand::SetFocus(opt_id) => {
                let surface = opt_id.and_then(|id| self.surface_for_window(id));
                let serial = SERIAL_COUNTER.next_serial();
                let kbd = self.keyboard.clone();
                kbd.set_focus(self, surface.clone(), serial);
                if let Some(s) = surface {
                    let _ = self.tx.send(BackendEvent::FocusIn {
                        event: self.window_id_from_surface(&s),
                    });
                }
            }
            CompositorCommand::StartMoveGrab(id) => {
                if let Some(surface) = self.surface_for_window(id) {
                    let window = self
                        .space
                        .lock()
                        .unwrap()
                        .elements()
                        .find(|w| {
                            w.toplevel()
                                .map(|t| t.wl_surface() == &surface)
                                .unwrap_or(false)
                        })
                        .cloned();

                    if let Some(window) = window {
                        if let Some(start_data) = self.pointer.grab_start_data() {
                            let serial = SERIAL_COUNTER.next_serial();
                            let initial_location = self
                                .space
                                .lock()
                                .unwrap()
                                .element_location(&window)
                                .unwrap();
                            let grab = PointerMoveSurfaceGrab {
                                start_data,
                                window,
                                initial_window_location: initial_location,
                            };
                            let ptr = self.pointer.clone();
                            ptr.set_grab(self, grab, serial, Focus::Clear);
                        }
                    }
                }
            }
        }
    }

    pub fn handle_keyboard_input<B: InputBackend>(&mut self, event: &B::KeyboardKeyEvent) {
        let state = event.state();
        let serial = SERIAL_COUNTER.next_serial();
        let time = event.time();

        let tx = self.tx.clone();

        let keyboard = self.keyboard.clone();
        keyboard.input(
            self,
            event.key_code(),
            state,
            serial,
            time as u32,
            move |_, modifiers, keysym: KeysymHandle<'_>| {
                let jwm_mods = smithay_mods_to_jwm_mods(*modifiers);

                for key_config in crate::config::CONFIG.get_keys().iter() {
                    let jwm_mask = key_config.mask & !(Mods::NUMLOCK | Mods::CAPS);

                    if jwm_mods == jwm_mask && keysym.modified_sym() == key_config.key_sym.into() {
                        if state == KeyState::Pressed {
                            let _ = tx.send(BackendEvent::WmKeyboardShortcut {
                                keysym: keysym.modified_sym().into(),
                                mods: jwm_mods,
                            });
                        }
                        return FilterResult::Intercept(true);
                    }
                }
                FilterResult::Forward
            },
        );
    }

    fn surface_for_window(&self, id: WindowId) -> Option<WlSurface> {
        self.registry
            .lock()
            .unwrap()
            .windows
            .get(&id.0)?
            .wl_surface
            .clone()
    }

    fn window_id_from_surface(&self, s: &WlSurface) -> WindowId {
        self.registry
            .lock()
            .unwrap()
            .windows
            .iter()
            .find(|(_, rec)| rec.wl_surface.as_ref() == Some(s))
            .map(|(id, _)| WindowId(*id))
            .unwrap_or(WindowId(0))
    }
}

fn smithay_mods_to_jwm_mods(s_mods: ModifiersState) -> Mods {
    let mut j_mods = Mods::empty();
    if s_mods.shift {
        j_mods |= Mods::SHIFT;
    }
    if s_mods.ctrl {
        j_mods |= Mods::CONTROL;
    }
    if s_mods.alt {
        j_mods |= Mods::ALT;
    }
    if s_mods.logo {
        j_mods |= Mods::SUPER;
    }
    j_mods
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
            if let Some(window) = space_guard.elements().find(|w| {
                w.toplevel()
                    .map(|t| t.wl_surface() == &root)
                    .unwrap_or(false)
            }) {
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
        space_guard.map_element(window.clone(), (50, 50), true);

        surface.with_pending_state(|pending| {
            pending.size = Some(smithay::utils::Size::from((800, 600)));
        });
        surface.send_configure();

        let mut reg = self.registry.lock().unwrap();
        let new_id = reg.windows.keys().max().copied().unwrap_or(0) + 1;
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
    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}
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
}

pub struct WaylandEventSource {
    event_rx: crossbeam_channel::Receiver<BackendEvent>,
    command_tx: smithay::reexports::calloop::channel::Sender<CompositorCommand>,
    registry: Arc<Mutex<WaylandRegistry>>,
    space: Arc<Mutex<Space<Window>>>,
}

impl WaylandEventSource {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (command_tx, command_rx) = channel();
        let (event_tx_crossbeam, event_rx_crossbeam) = crossbeam_channel::unbounded();

        let registry = Arc::new(Mutex::new(WaylandRegistry::new()));
        let space = Arc::new(Mutex::new(Space::default()));

        let registry_clone = registry.clone();
        let space_clone = space.clone();

        std::thread::spawn(move || {
            let mut event_loop: EventLoop<LoopData> = EventLoop::try_new().unwrap();
            let display: Display<JwmWlState> = Display::new().unwrap();
            let dh = display.handle();

            let (event_tx_for_loop, event_rx_for_loop) = channel();
            event_loop
                .handle()
                .insert_source(event_rx_for_loop, move |event, _, _| {
                    if let ChannelEvent::Msg(evt) = event {
                        // Send to the crossbeam channel for the main thread
                        let _ = event_tx_crossbeam.send(evt);
                    }
                })
                .unwrap();

            let state = JwmWlState::new(&dh, event_tx_for_loop, registry_clone, space_clone);
            let mut data = LoopData { state };

            event_loop
                .handle()
                .insert_source(command_rx, |event, _, data| {
                    if let ChannelEvent::Msg(cmd) = event {
                        data.state.process_command(cmd);
                    }
                })
                .unwrap();

            event_loop
                .handle()
                .insert_source(
                    ListeningSocketSource::new_auto().unwrap(),
                    move |client_stream, _, data| {
                        data.state
                            .display_handle
                            .insert_client(client_stream, Arc::new(ClientState::default()))
                            .unwrap();
                    },
                )
                .unwrap();

            event_loop
                .handle()
                .insert_source(
                    Generic::new(display, Interest::READ, Mode::Level),
                    |_, display, data| {
                        unsafe {
                            display.get_mut().dispatch_clients(&mut data.state).unwrap();
                        }
                        Ok(PostAction::Continue)
                    },
                )
                .unwrap();

            event_loop.run(None, &mut data, |_| {}).unwrap();
        });

        Ok(Self {
            event_rx: event_rx_crossbeam,
            command_tx,
            registry,
            space,
        })
    }

    pub fn command_sender(
        &self,
    ) -> smithay::reexports::calloop::channel::Sender<CompositorCommand> {
        self.command_tx.clone()
    }

    pub fn registry(&self) -> Arc<Mutex<WaylandRegistry>> {
        self.registry.clone()
    }

    pub fn space(&self) -> Arc<Mutex<Space<Window>>> {
        self.space.clone()
    }
}

impl EventSource for WaylandEventSource {
    fn poll_event(&mut self) -> Result<Option<BackendEvent>, Box<dyn std::error::Error>> {
        Ok(self.event_rx.try_recv().ok())
    }
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
