// src/backend/wayland/event_source.rs
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Receiver, Sender};

use super::grabs::PointerMoveSurfaceGrab;
use super::window_ops::{WaylandRegistry, WindowRecord};
use crate::backend::api::{BackendEvent, EventSource, WindowId};
use crate::backend::common_define::keys as k;
use crate::backend::common_define::Mods;

use smithay::input::pointer::Focus;
use smithay::input::Seat;
use smithay::reexports::wayland_server::protocol::{wl_seat::WlSeat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{
    backend::{ClientData, ClientId, DisconnectReason},
    Client, Display, DisplayHandle,
};
use smithay::utils::SERIAL_COUNTER;
use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{keyboard::ModifiersState, SeatHandler, SeatState},
    output::{Mode, Output, PhysicalProperties, Subpixel},
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

// 定义从 JWM Core 发往此线程的命令
#[derive(Debug)]
pub enum CompositorCommand {
    SetFocus(Option<WindowId>),
    StartMoveGrab(WindowId),
    // 更多命令可以加在这里
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
    space: Arc<Mutex<Space<Window>>>,
    registry: Arc<Mutex<WaylandRegistry>>,
    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    data_device_state: DataDeviceState,
    popups: PopupManager,
    seat_state: SeatState<JwmWlState>,
    pub seat: Arc<Mutex<Seat<JwmWlState>>>,
    pub keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
    pub pointer: smithay::input::pointer::PointerHandle<JwmWlState>,
    tx: Sender<BackendEvent>, // 发送事件回 JWM
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

    // 命令处理逻辑
    pub fn process_command(&mut self, cmd: CompositorCommand) {
        match cmd {
            CompositorCommand::SetFocus(opt_id) => {
                let surface = opt_id.and_then(|id| self.surface_for_window(id));
                let serial = SERIAL_COUNTER.next_serial();
                self.seat
                    .get_keyboard()
                    .unwrap()
                    .set_focus(self, surface.clone(), serial);

                if let Some(s) = surface {
                    let _ = self.tx.send(BackendEvent::FocusIn {
                        event: self.window_id_from_surface(&s),
                    });
                }
            }
            CompositorCommand::StartMoveGrab(id) => {
                if let Some(surface) = self.surface_for_window(id) {
                    if let Some(window) = self
                        .space
                        .lock()
                        .unwrap()
                        .elements()
                        .find(|w| w.toplevel().unwrap().wl_surface() == &surface)
                        .cloned()
                    {
                        if let Some(start_data) = self.pointer.grab_start_data() {
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

                            self.pointer
                                .set_grab(self, grab, start_data.serial, Focus::Clear);
                        }
                    }
                }
            }
        }
    }

    // 键盘输入处理逻辑 (将被 libinput 等后端的事件循环调用)
    pub fn handle_keyboard_input(&mut self, event: smithay::input::keyboard::KeyboardKeyEvent) {
        let keysym = event.key_symbol();
        let state = event.state();

        let mods = smithay_mods_to_jwm_mods(self.keyboard.modifiers_state());

        for key_config in crate::config::CONFIG.get_keys().iter() {
            let jwm_mask = key_config.mask & !(Mods::NUMLOCK | Mods::CAPS);
            if mods == jwm_mask && keysym == key_config.key_sym {
                if state == smithay::input::keyboard::KeyState::Pressed {
                    // 找到了！发送事件回 JWM 主线程
                    let _ = self.tx.send(BackendEvent::WmKeyboardShortcut {
                        keysym: key_config.key_sym,
                        mods: key_config.mask,
                    });
                }
                // 无论按下还是抬起，我们都消耗掉这个事件，不发给客户端
                return;
            }
        }

        // 如果没有匹配的快捷键，将事件转发给当前聚焦的客户端
        self.keyboard.input(
            self,
            event.key_code(),
            state,
            SERIAL_COUNTER.next_serial(),
            event.time(),
            |_, _, _| true,
        );
    }

    // 辅助函数
    fn surface_for_window(&self, id: WindowId) -> Option<WlSurface> {
        self.registry
            .lock()
            .unwrap()
            .windows
            .get(&id.0)
            .and_then(|rec| rec.wl_surface.clone())
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

// 辅助函数
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

// --- Handler Trait 实现 ---

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
            state.geometry = positioner.get_geometry();
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
    // tx 不再需要，因为事件直接在 JwmWlState 中发送
    // tx: Sender<BackendEvent>,
    command_tx: Sender<CompositorCommand>,
    registry: Arc<Mutex<WaylandRegistry>>,
    space: Arc<Mutex<Space<Window>>>,
    seat: Arc<Mutex<Seat<JwmWlState>>>,
    pointer: smithay::input::pointer::PointerHandle<JwmWlState>,
    keyboard: smithay::input::keyboard::KeyboardHandle<JwmWlState>,
}

impl WaylandEventSource {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (event_tx, event_rx) = unbounded();
        let (command_tx, command_rx) = unbounded::<CompositorCommand>();

        let registry = Arc::new(Mutex::new(WaylandRegistry::new()));
        let space_arc = Arc::new(Mutex::new(Space::default()));

        let (seat_arc, pointer_handle, keyboard_handle) = {
            let tx_clone = event_tx.clone();
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

                // 添加命令监听
                event_loop
                    .handle()
                    .insert_source(
                        Generic::new(command_rx, Interest::READ, Mode::Level),
                        |_, rx, data| {
                            while let Ok(cmd) = rx.try_recv() {
                                data.state.process_command(cmd);
                            }
                            Ok(PostAction::Continue)
                        },
                    )
                    .unwrap();

                let seat_arc = data.state.seat.clone();
                let pointer_handle = data.state.pointer.clone();
                let keyboard_handle = data.state.keyboard.clone();
                let _ = seat_sender.send((seat_arc, pointer_handle, keyboard_handle));

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
            rx: event_rx,
            // tx: event_tx,
            command_tx,
            registry,
            space: space_arc,
            seat: seat_arc,
            pointer: pointer_handle,
            keyboard: keyboard_handle,
        })
    }

    pub fn command_sender(&self) -> Sender<CompositorCommand> {
        self.command_tx.clone()
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
