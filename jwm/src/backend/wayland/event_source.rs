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

/// 客户端数据（至少包含 CompositorClientState）
#[derive(Default)]
struct ClientState {
    compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Smithay 的全局状态类型（D），持有必要的协议状态与空间
pub struct JwmWlState {
    display_handle: DisplayHandle,
    space: Space<Window>,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    shm_state: ShmState,
    output_manager_state: OutputManagerState,
    seat_state: SeatState<JwmWlState>,
    seat: Seat<JwmWlState>,
    data_device_state: DataDeviceState,
    popups: PopupManager,

    // 向上层 JWM 事件源发送通道
    tx: Sender<BackendEvent>,
    // 窗口注册表（Wayland WindowId 映射）
    registry: Arc<Mutex<WaylandRegistry>>,
}

impl JwmWlState {
    fn new(
        dh: &DisplayHandle,
        tx: Sender<BackendEvent>,
        registry: Arc<Mutex<WaylandRegistry>>,
    ) -> Self {
        // 初始化各协议状态
        let compositor_state = CompositorState::new::<JwmWlState>(dh);
        let xdg_shell_state = XdgShellState::new::<JwmWlState>(dh);
        let shm_state = ShmState::new::<JwmWlState>(dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<JwmWlState>(dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<JwmWlState>(dh);
        let popups = PopupManager::default();

        // 新建 seat，并声明键盘与指针
        let mut seat: Seat<JwmWlState> = seat_state.new_wl_seat(dh, "jwm-wayland");
        let _ = seat.add_keyboard(Default::default(), 200, 25);
        seat.add_pointer();

        Self {
            display_handle: dh.clone(),
            space: Space::default(),

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            seat,
            data_device_state,
            popups,

            tx,
            registry,
        }
    }
}

/// CompositorHandler：提交处理（buffer/on_commit），与 Window 同步
impl CompositorHandler for JwmWlState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        // 处理 shm buffer 提交
        on_commit_buffer_handler::<Self>(surface);

        // 同步根 surface 的 window 状态
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == &root)
            {
                window.on_commit();
            }
        }

        // 可在此根据几何/状态变化向 tx 发送 BackendEvent::ConfigureNotify 等（后续扩展）
    }
}

/// ShmHandler/BufferHandler：最小实现即可
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

/// SeatHandler：焦点改变时更新 data device 焦点
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

/// OutputHandler：最小实现即可（所有输出相关通过 OutputManagerState）
impl OutputHandler for JwmWlState {}

/// DataDeviceHandler + SelectionHandler
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

/// XdgShellHandler：补齐 new_toplevel/new_popup/grab/reposition_request
impl XdgShellHandler for JwmWlState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        // 简单映射到 (0,0)
        self.space.map_element(window.clone(), (0, 0), false);

        // 记录到注册表，并发 MapRequest 事件
        let mut reg = self.registry.lock().unwrap();
        let new_id = (reg.windows.len() as u64) + 1;
        reg.windows.insert(
            new_id,
            WindowRecord {
                id: new_id,
                x: 0,
                y: 0,
                // 几何可能为 0，后续 commit/配置可更新
                w: window.geometry().size.w,
                h: window.geometry().size.h,
                border: 0,
            },
        );

        let _ = self.tx.send(BackendEvent::MapRequest {
            window: WindowId(new_id),
        });
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        // 最小实现：跟踪 popup
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: smithay::utils::Serial) {
        // TODO: popup grabs，如需特殊处理可扩展
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        // 根据 positioner 更新 popup 的 pending state 并回执
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        surface.send_repositioned(token);
    }
}

// 必要的 delegate 宏，提供 GlobalDispatch/Dispatch 的实现
delegate_compositor!(JwmWlState);
delegate_shm!(JwmWlState);
delegate_seat!(JwmWlState);
delegate_output!(JwmWlState);
delegate_xdg_shell!(JwmWlState);
delegate_data_device!(JwmWlState);

/// 事件循环数据，避免将 state/display_handle 捕获进闭包导致生命周期/移动问题
struct LoopData {
    state: JwmWlState,
    display_handle: DisplayHandle,
}

/// Wayland 事件源：后台线程运行 smithay+calloop，抛出 BackendEvent
pub struct WaylandEventSource {
    rx: Receiver<BackendEvent>,
    tx: Sender<BackendEvent>,
    registry: Arc<Mutex<WaylandRegistry>>,
    // 控制器占位（如需扩展输入桥接，可在其它模块实现）
    pointer: super::input_ops::PointerController,
    keyboard: super::key_ops::KeyboardController,
    cursor: super::cursor::CursorController,
}

impl WaylandEventSource {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded();
        let registry = Arc::new(Mutex::new(WaylandRegistry::new()));

        let pointer = super::input_ops::PointerController::new();
        let keyboard = super::key_ops::KeyboardController::new();
        let cursor = super::cursor::CursorController::new();

        // 后台线程运行 smithay display + calloop
        {
            let tx_clone = tx.clone();
            let reg_clone = registry.clone();

            std::thread::spawn(move || {
                use smithay::reexports::calloop::{
                    generic::Generic, EventLoop, Interest, Mode, PostAction,
                };

                // 事件循环与 display
                let mut event_loop: EventLoop<LoopData> = EventLoop::try_new().unwrap();
                let display: Display<JwmWlState> = Display::new().unwrap();
                let dh = display.handle();

                // 创建状态
                let state = JwmWlState::new(&dh, tx_clone, reg_clone);
                let mut data = LoopData {
                    state,
                    display_handle: dh.clone(),
                };

                // 监听 Wayland socket，插入客户端
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

                // 将 display 插入事件循环，分发客户端请求
                event_loop
                    .handle()
                    .insert_source(
                        Generic::new(display, Interest::READ, Mode::Level),
                        |_, display, data| {
                            // Safety: 不 drop display
                            unsafe {
                                display.get_mut().dispatch_clients(&mut data.state).unwrap();
                            }
                            Ok(PostAction::Continue)
                        },
                    )
                    .unwrap();

                // 运行事件循环
                event_loop
                    .run(None, &mut data, move |_| {
                        // 可在此进行心跳/渲染请求等扩展
                    })
                    .unwrap();
            });
        }

        Ok(Self {
            rx,
            tx,
            registry,
            pointer,
            keyboard,
            cursor,
        })
    }

    pub fn registry(&self) -> Arc<Mutex<WaylandRegistry>> {
        self.registry.clone()
    }
    pub fn pointer_controller(&self) -> super::input_ops::PointerController {
        self.pointer.clone()
    }
    pub fn keyboard_controller(&self) -> super::key_ops::KeyboardController {
        self.keyboard.clone()
    }
    pub fn cursor_controller(&self) -> super::cursor::CursorController {
        self.cursor.clone()
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
