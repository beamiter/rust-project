// src/backend/wayland/event_source.rs
use std::sync::{Arc, Mutex};
use std::time::Duration;

use calloop::channel::channel;
use smithay::{
    backend::{
        // FIX: Use correct imports for smithay 0.7.0
        graphics::{
            egl::{EGLContext, EGLDisplay},
            gles2::Gles2Renderer,
        },
        input::{InputEvent, KeyState, KeyboardKeyEvent},
        winit::{self, WinitEvent, WinitInputBackend},
    },
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, Space, Window},
    input::{
        keyboard::{FilterResult, KeysymHandle, ModifiersState},
        pointer::{ButtonEvent, CursorIcon, CursorImageStatus, Focus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::{
            channel::Event as ChannelEvent, timer::Timer, Dispatcher, EventLoop, PostAction, Source,
        },
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_seat::WlSeat, wl_surface::WlSurface},
            Client, Display, DisplayHandle,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Serial, Transform, SERIAL_COUNTER},
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

use super::{
    cursor::{Cursor, PointerElement},
    grabs::{PointerMoveSurfaceGrab, PointerResizeSurfaceGrab, ResizeEdge},
    render,
    window_ops::{WaylandRegistry, WindowRecord},
};
use crate::backend::{
    api::{BackendEvent, EventSource, WindowId},
    common_define::Mods,
};
use smithay::backend::renderer::damage::OutputDamageTracker;

#[derive(Debug)]
pub enum CompositorCommand {
    SetFocus(Option<WindowId>),
    StartMoveGrab(WindowId),
    StartResizeGrab { window: WindowId, edges: ResizeEdge },
    SetCursor(String),
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
    pub display_handle: DisplayHandle,
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
    pub pointer: PointerHandle<JwmWlState>,
    tx: smithay::reexports::calloop::channel::Sender<BackendEvent>,

    cursor: Cursor,
    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: Arc<Mutex<CursorImageStatus>>,
    pub pointer_element: Arc<Mutex<PointerElement>>,
    clock: Clock<Monotonic>,
}

// FIX: A simplified compositor state for the event loop
struct WlCompositor {
    display: Display<JwmWlState>,
    state: JwmWlState,
    // Rendering components
    renderer: Gles2Renderer,
    damage_tracker: OutputDamageTracker,
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
            cursor: Cursor::load(),
            pointer_location: (0.0, 0.0).into(),
            cursor_status: Arc::new(Mutex::new(CursorImageStatus::default_named())),
            pointer_element: Arc::new(Mutex::new(PointerElement::default())),
            clock: Clock::new(),
        }
    }

    pub fn process_command(&mut self, cmd: CompositorCommand) {
        match cmd {
            CompositorCommand::SetCursor(name) => {
                let seat = self.seat.clone();
                // FIX: `from_name` returns a Result in 0.7.0
                let icon = CursorIcon::from_name(&name).unwrap_or_default();
                self.cursor_image(&seat, CursorImageStatus::Named(icon));
            }
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
                let window = if let Some(surface) = self.surface_for_window(id) {
                    self.space
                        .lock()
                        .unwrap()
                        .elements()
                        .find(|w| {
                            w.toplevel()
                                .map(|t| t.wl_surface() == &surface)
                                .unwrap_or(false)
                        })
                        .cloned()
                } else {
                    None
                };

                if let Some(window) = window {
                    if let Some(start_data) = self.pointer.grab_start_data() {
                        let serial = SERIAL_COUNTER.next_serial();
                        let initial_location = {
                            self.space
                                .lock()
                                .unwrap()
                                .element_location(&window)
                                .unwrap()
                        };
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

            CompositorCommand::StartResizeGrab { window: id, edges } => {
                let window = if let Some(surface) = self.surface_for_window(id) {
                    self.space
                        .lock()
                        .unwrap()
                        .elements()
                        .find(|w| {
                            w.toplevel()
                                .map(|t| t.wl_surface() == &surface)
                                .unwrap_or(false)
                        })
                        .cloned()
                } else {
                    None
                };

                if let Some(window) = window {
                    if let Some(start_data) = self.pointer.grab_start_data() {
                        let serial = SERIAL_COUNTER.next_serial();
                        let (initial_window_location, initial_window_size) = {
                            let space = self.space.lock().unwrap();
                            (
                                space.element_location(&window).unwrap(),
                                window.geometry().size,
                            )
                        };
                        let grab = PointerResizeSurfaceGrab {
                            start_data,
                            window,
                            edges,
                            initial_window_location,
                            initial_window_size,
                            last_window_size: initial_window_size,
                        };
                        let ptr = self.pointer.clone();
                        ptr.set_grab(self, grab, serial, Focus::Clear);
                    }
                }
            }
        }
    }
    // ...
    // The rest of the impl JwmWlState block is mostly fine
    // ...
}

// ... All the handler impls for JwmWlState are fine ...

// The event source part needs a major rewrite
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
            let mut event_loop: EventLoop<WlCompositor> = EventLoop::try_new().unwrap();
            let display: Display<JwmWlState> = Display::new().unwrap();

            // Setup the backend
            let (backend, mut winit_loop) = winit::init().unwrap();
            let size = backend.window_size();

            // Setup the renderer
            let egl_display = EGLDisplay::new(backend.display(), None).unwrap();
            let egl_context = EGLContext::new(&egl_display, None).unwrap();
            let renderer = unsafe { Gles2Renderer::new(egl_context, None).unwrap() };

            // Setup the state
            let (event_tx_for_loop, _event_rx_for_loop) = channel();
            let state = JwmWlState::new(
                &display.handle(),
                event_tx_for_loop,
                registry_clone,
                space_clone,
            );

            // Create a Smithay output for the winit window
            let output = Output::new(
                "winit".to_string(),
                PhysicalProperties {
                    size: (0, 0).into(),
                    subpixel: Subpixel::Unknown,
                    make: "Smithay".into(),
                    model: "Winit".into(),
                },
            );
            let _global = output.create_global(&display.handle());
            let mode = Mode {
                size,
                refresh: 60_000,
            };
            output.change_current_state(
                Some(mode),
                Some(Transform::Normal),
                None,
                Some((0, 0).into()),
            );
            output.set_preferred(mode);
            state.space.lock().unwrap().map_output(&output, (0, 0));

            let mut compositor = WlCompositor {
                display,
                state,
                renderer,
                damage_tracker: OutputDamageTracker::default(),
            };

            // This channel forwards events from the compositor thread to the main jwm thread
            let _ = event_loop
                .handle()
                .insert_source(command_rx, move |event, _, data| {
                    if let ChannelEvent::Msg(cmd) = event {
                        data.state.process_command(cmd);
                    }
                });

            // Winit event source
            let _ =
                event_loop
                    .handle()
                    .insert_source(winit_loop, move |event, _, data| match event {
                        WinitEvent::Resized { size, .. } => {
                            let space = data.state.space.lock().unwrap();
                            if let Some(output) = space.outputs().find(|o| o.name() == "winit") {
                                output.change_current_state(
                                    Some(Mode {
                                        size,
                                        refresh: 60_000,
                                    }),
                                    None,
                                    None,
                                    None,
                                );
                            }
                        }
                        WinitEvent::Input(event) => match event {
                            InputEvent::Keyboard { event } => {
                                data.state.keyboard.input(
                                    &mut data.state,
                                    event.key_code(),
                                    event.state(),
                                    SERIAL_COUNTER.next_serial(),
                                    event.time(),
                                    |_, _, _| FilterResult::Forward,
                                );
                            }
                            InputEvent::PointerMotion { event } => {
                                let pointer = data.state.pointer.clone();
                                data.state.pointer_location += event.delta();
                                let under = data
                                    .state
                                    .space
                                    .lock()
                                    .unwrap()
                                    .element_under(data.state.pointer_location);
                                pointer.motion(
                                    &mut data.state,
                                    under.map(|(w, l)| (w.wl_surface().clone(), l)),
                                    &event,
                                    event.time(),
                                );
                            }
                            InputEvent::PointerButton { event } => {
                                let pointer = data.state.pointer.clone();
                                pointer.button(
                                    &mut data.state,
                                    &event,
                                    SERIAL_COUNTER.next_serial(),
                                    event.time(),
                                );
                            }
                            _ => {}
                        },
                        WinitEvent::Redraw => {
                            let space = data.state.space.lock().unwrap();
                            if let Some(output) =
                                space.outputs().find(|o| o.name() == "winit").cloned()
                            {
                                let _ = render::render_output(
                                    &output,
                                    &space,
                                    &data.state.pointer_element.lock().unwrap(),
                                    data.state.pointer_location,
                                    &mut data.renderer,
                                    &mut data.damage_tracker,
                                    0,
                                );
                            }
                        }
                        _ => (),
                    });

            // Wayland socket
            let source = ListeningSocketSource::new_auto().unwrap();
            let _ = event_loop
                .handle()
                .insert_source(source, move |client_stream, _, data| {
                    if let Err(err) = data
                        .display
                        .handle()
                        .insert_client(client_stream, Arc::new(ClientState::default()))
                    {
                        eprintln!("Error adding wayland client: {}", err);
                    }
                });

            // Display dispatcher
            let _ = event_loop.handle().insert_source(
                Dispatcher::new(compositor.display.clone(), |_, _| {}),
                |_, _, _| {},
            );

            // Timer for cursor animation
            let timer = Timer::new().unwrap();
            let _ = event_loop
                .handle()
                .insert_source(timer.clone(), |_, _, _| {});

            let mut last_cursor_update = std::time::Instant::now();

            event_loop
                .run(None, &mut compositor, |data| {
                    // Dispatch wayland clients
                    let _ = data.display.dispatch_clients(&mut data.state);

                    // Cursor animation timer
                    if last_cursor_update.elapsed() > Duration::from_millis(20) {
                        let _ = data
                            .state
                            .seat
                            .get_pointer()
                            .map(|p| p.frame(&mut data.state));
                        last_cursor_update = std::time::Instant::now();
                    }
                })
                .unwrap();
        });

        Ok(Self {
            event_rx: event_rx_crossbeam,
            command_tx,
            registry,
            space,
        })
    }
    // ... rest of WaylandEventSource impl ...
}

// ... rest of the file ...
