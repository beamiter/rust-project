use crate::backend::api::{
    Backend, BackendEvent, Capabilities, ColorAllocator, CursorProvider, EventHandler, HitTarget,
    InputOps, KeyOps, OutputInfo, OutputOps, PropertyOps, ResizeEdge, ScreenInfo, WindowOps,
};
use crate::backend::common_define::{KeySym, Mods, OutputId, WindowId};
use crate::backend::error::BackendError;
use crate::backend::wayland::state::JwmWaylandState;
use crate::backend::{wayland_dummy_ops, wayland_key_ops};
use crate::config::CONFIG;

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use smithay::backend::allocator::dmabuf::DmabufAllocator;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags};
use smithay::backend::allocator::Modifier;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::input::{
    AbsolutePositionEvent, Event as InputEventExt, InputBackend, InputEvent, KeyboardKeyEvent,
    PointerButtonEvent, PointerMotionEvent,
};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{AsRenderElements, Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::{Bind, Color32F, ImportAll, ImportEgl, ImportMem};
use smithay::backend::x11::{Window, WindowBuilder, X11Backend, X11Event, X11Surface};
use smithay::desktop::layer_map_for_output;
use smithay::desktop::space::SurfaceTree;
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::input::keyboard::{FilterResult, ModifiersState};
use smithay::input::pointer::{ButtonEvent, MotionEvent};
use smithay::output::{Mode as WlMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::calloop::channel::{self, Sender};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{EventLoop, Interest, Mode, PostAction};
use smithay::reexports::gbm;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{self, Display, DisplayHandle, Resource};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::utils::{DeviceFd, Logical, Physical, Point, Rectangle, Scale, SERIAL_COUNTER as SCOUNTER};
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};
use smithay::wayland::shell::wlr_layer::{KeyboardInteractivity, Layer as WlrLayer};

smithay::backend::renderer::element::render_elements! {
    pub X11RenderElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Solid=SolidColorRenderElement,
}

#[derive(Clone)]
struct SendWrapper<T>(T);
unsafe impl<T> Send for SendWrapper<T> {}

impl<T> std::ops::Deref for SendWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for SendWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct SharedState {
    pointer_x: f64,
    pointer_y: f64,
    mods_state: u16,
    /// Cached key bindings (mods, keysym) for key event suppression.
    key_bindings: Vec<(Mods, KeySym)>,
    /// xkb keycode (0..=255) -> base (unmodified) keysym.
    keysym_table: Vec<KeySym>,
    /// xkb keycodes that were intercepted on press and should be intercepted on release.
    suppressed_keycodes: HashSet<u8>,

    outputs: Vec<OutputInfo>,
    output_key_to_id: HashMap<u64, OutputId>,
    next_output_raw: u64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            pointer_x: 0.0,
            pointer_y: 0.0,
            mods_state: 0,
            key_bindings: Vec::new(),
            keysym_table: vec![0; 256],
            suppressed_keycodes: HashSet::new(),
            outputs: Vec::new(),
            output_key_to_id: HashMap::new(),
            next_output_raw: 0,
        }
    }
}

fn mods_from_smithay(mods: &ModifiersState) -> Mods {
    let mut out = Mods::empty();

    if mods.shift {
        out |= Mods::SHIFT;
    }
    if mods.ctrl {
        out |= Mods::CONTROL;
    }
    if mods.alt {
        out |= Mods::ALT;
    }
    if mods.logo {
        out |= Mods::SUPER;
    }
    if mods.caps_lock {
        out |= Mods::CAPS;
    }
    if mods.num_lock {
        out |= Mods::NUMLOCK;
    }

    out
}

struct WaylandOutputOps {
    shared: Arc<Mutex<SharedState>>,
}

impl OutputOps for WaylandOutputOps {
    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        self.shared.lock().unwrap().outputs.clone()
    }

    fn screen_info(&self) -> ScreenInfo {
        let shared = self.shared.lock().unwrap();
        let mut w = 0i32;
        let mut h = 0i32;
        for o in &shared.outputs {
            w = w.max(o.x + o.width);
            h = h.max(o.y + o.height);
        }
        if w == 0 {
            w = 1280;
        }
        if h == 0 {
            h = 720;
        }
        ScreenInfo { width: w, height: h }
    }

    fn output_at(&self, x: i32, y: i32) -> Option<OutputId> {
        let shared = self.shared.lock().unwrap();
        shared
            .outputs
            .iter()
            .find(|o| x >= o.x && y >= o.y && x < (o.x + o.width) && y < (o.y + o.height))
            .map(|o| o.id)
    }
}

struct WaylandWindowOps {
    state: SendWrapper<*mut JwmWaylandState>,
    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,
}

unsafe impl Send for WaylandWindowOps {}

impl WaylandWindowOps {
    unsafe fn with_state_mut<R>(&self, f: impl FnOnce(&mut JwmWaylandState) -> R) -> R {
        unsafe { f(&mut *self.state.0) }
    }

    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }
}

impl WindowOps for WaylandWindowOps {
    fn set_position(&self, win: WindowId, x: i32, y: i32) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(geo) = state.window_geometry.get_mut(&win) {
                    let bw = geo.border as i32;
                    geo.x = x + bw;
                    geo.y = y + bw;
                }
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn configure(
        &self,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        border: u32,
    ) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.pending_initial_configure.remove(&win);
                let bw = border as i32;
                state.window_geometry.insert(
                    win,
                    crate::backend::api::Geometry {
                        x: x + bw,
                        y: y + bw,
                        w,
                        h,
                        border,
                    },
                );
                if let Some(toplevel) = state.try_lookup_toplevel(win) {
                    toplevel.with_pending_state(|s| {
                        s.size = Some((w as i32, h as i32).into());
                    });
                    toplevel.send_configure();
                }
                state.reconstrain_popups_for_toplevel(win);
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn set_decoration_style(
        &self,
        _win: WindowId,
        _border_width: u32,
        _border_color: crate::backend::common_define::Pixel,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn raise_window(&self, win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(pos) = state.window_stack.iter().position(|w| *w == win) {
                    state.window_stack.remove(pos);
                    state.window_stack.push(win);
                }
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn map_window(&self, win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.mapped_windows.insert(win);
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn unmap_window(&self, win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.mapped_windows.remove(&win);
                state.needs_redraw = true;
            });
        }
        self.request_flush();
        Ok(())
    }

    fn close_window(&self, win: WindowId) -> Result<crate::backend::api::CloseResult, BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(toplevel) = state.try_lookup_toplevel(win) {
                    toplevel.send_close();
                }
            });
        }
        self.request_flush();
        Ok(crate::backend::api::CloseResult::Graceful)
    }

    fn set_input_focus(&self, win: WindowId) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(surface) = state.surface_for_window(win) {
                    if let Some(kbd) = state.seat.get_keyboard() {
                        kbd.set_focus(state, Some(surface), SCOUNTER.next_serial());
                    }
                }
            });
        }
        self.request_flush();
        Ok(())
    }

    fn set_input_focus_root(&self) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                if let Some(kbd) = state.seat.get_keyboard() {
                    kbd.set_focus(state, None, SCOUNTER.next_serial());
                }
            });
        }
        self.request_flush();
        Ok(())
    }

    fn get_window_attributes(
        &self,
        win: WindowId,
    ) -> Result<crate::backend::api::WindowAttributes, BackendError> {
        let viewable = unsafe { self.with_state_mut(|state| state.mapped_windows.contains(&win)) };
        Ok(crate::backend::api::WindowAttributes {
            override_redirect: false,
            map_state_viewable: viewable,
        })
    }

    fn get_geometry(&self, win: WindowId) -> Result<crate::backend::api::Geometry, BackendError> {
        let geo = unsafe { self.with_state_mut(|state| state.window_geometry.get(&win).copied()) };
        let mut geo = geo.unwrap_or_default();
        let bw = geo.border as i32;
        geo.x = geo.x - bw;
        geo.y = geo.y - bw;
        Ok(geo)
    }

    fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError> {
        unsafe { Ok(self.with_state_mut(|state| state.window_stack.clone())) }
    }

    fn flush(&self) -> Result<(), BackendError> {
        self.request_flush();
        Ok(())
    }

    fn kill_client(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn apply_window_changes(
        &self,
        win: WindowId,
        changes: crate::backend::api::WindowChanges,
    ) -> Result<(), BackendError> {
        let geo = self.get_geometry(win)?;
        let x = changes.x.unwrap_or(geo.x);
        let y = changes.y.unwrap_or(geo.y);
        let w = changes.width.unwrap_or(geo.w);
        let h = changes.height.unwrap_or(geo.h);
        let border = changes.border_width.unwrap_or(geo.border);
        self.configure(win, x, y, w, h, border)
    }
}

struct WaylandPropertyOps {
    state: SendWrapper<*mut JwmWaylandState>,
    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,
}

unsafe impl Send for WaylandPropertyOps {}

impl WaylandPropertyOps {
    unsafe fn with_state_mut<R>(&self, f: impl FnOnce(&mut JwmWaylandState) -> R) -> R {
        unsafe { f(&mut *self.state.0) }
    }

    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }
}

impl PropertyOps for WaylandPropertyOps {
    fn get_title(&self, win: WindowId) -> String {
        unsafe {
            self.with_state_mut(|state| {
                let title = state.window_title.get(&win).cloned().unwrap_or_default();
                if !title.is_empty() {
                    return title;
                }
                let app_id = state.window_app_id.get(&win).cloned().unwrap_or_default();
                if !app_id.is_empty() {
                    return app_id;
                }
                "Wayland Window".to_string()
            })
        }
    }

    fn get_class(&self, win: WindowId) -> (String, String) {
        let app_id = unsafe {
            self.with_state_mut(|state| state.window_app_id.get(&win).cloned().unwrap_or_default())
        };
        let value = if app_id.is_empty() { "app".to_string() } else { app_id };
        (value.clone(), value)
    }

    fn get_window_types(&self, win: WindowId) -> Vec<crate::backend::api::WindowType> {
        // Best-effort classification so JWM can treat status bars/docks correctly.
        let (title, app_id, layer_info) = unsafe {
            self.with_state_mut(|state| {
                (
                    state.window_title.get(&win).cloned().unwrap_or_default(),
                    state.window_app_id.get(&win).cloned().unwrap_or_default(),
                    state.window_layer_info.get(&win).copied(),
                )
            })
        };

        if let Some(info) = layer_info {
            if info.exclusive_zone != 0 {
                return vec![crate::backend::api::WindowType::Dock];
            }
        }

        let cfg = crate::config::CONFIG.load();
        let bar_name = cfg.status_bar_name();
        if !bar_name.is_empty() && (title == bar_name || app_id == bar_name) {
            return vec![crate::backend::api::WindowType::Dock];
        }

        vec![crate::backend::api::WindowType::Normal]
    }

    fn get_layer_surface_info(&self, win: WindowId) -> Option<crate::backend::api::LayerSurfaceInfo> {
        unsafe { self.with_state_mut(|state| state.window_layer_info.get(&win).copied()) }
    }

    fn is_fullscreen(&self, win: WindowId) -> bool {
        unsafe { self.with_state_mut(|state| state.window_is_fullscreen.get(&win).copied()) }
            .unwrap_or(false)
    }

    fn set_fullscreen_state(&self, win: WindowId, on: bool) -> Result<(), BackendError> {
        unsafe {
            self.with_state_mut(|state| {
                state.window_is_fullscreen.insert(win, on);
                if let Some(toplevel) = state.try_lookup_toplevel(win) {
                    toplevel.with_pending_state(|s| {
                        if on {
                            s.states.set(xdg_toplevel::State::Fullscreen);
                        } else {
                            s.states.unset(xdg_toplevel::State::Fullscreen);
                            s.fullscreen_output = None;
                        }
                    });
                    toplevel.send_configure();
                }
            });
        }
        self.request_flush();
        Ok(())
    }

    fn transient_for(&self, win: WindowId) -> Option<WindowId> {
        unsafe {
            self.with_state_mut(|state| {
                let toplevel = state.toplevels.get(&win)?;
                let parent_surface = toplevel.parent()?;
                state.surface_to_window.get(&parent_surface.id()).copied()
            })
        }
    }

    fn get_wm_hints(&self, _win: WindowId) -> Option<crate::backend::api::WmHints> {
        None
    }

    fn set_urgent_hint(&self, _win: WindowId, _urgent: bool) -> Result<(), BackendError> {
        Ok(())
    }

    fn fetch_normal_hints(&self, _win: WindowId) -> Result<Option<crate::backend::api::NormalHints>, BackendError> {
        Ok(None)
    }

    fn set_window_strut_top(
        &self,
        _win: WindowId,
        _top: u32,
        _start_x: u32,
        _end_x: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn clear_window_strut(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn get_wm_state(&self, _win: WindowId) -> Result<i64, BackendError> {
        Ok(1)
    }

    fn set_wm_state(&self, _win: WindowId, _state: i64) -> Result<(), BackendError> {
        Ok(())
    }

    fn set_client_info_props(&self, _win: WindowId, _tags: u32, _monitor_num: u32) -> Result<(), BackendError> {
        Ok(())
    }
}

pub struct WaylandX11Backend {
    event_loop: SendWrapper<EventLoop<'static, JwmWaylandState>>,
    state: Box<JwmWaylandState>,
    #[allow(dead_code)]
    socket_name: Option<String>,

    pending_events: Arc<Mutex<VecDeque<BackendEvent>>>,
    flush_tx: Sender<()>,
    flush_pending: Arc<AtomicBool>,

    shared: Arc<Mutex<SharedState>>,
    exit_requested: Arc<AtomicBool>,

    output: Output,
    window: Window,
    x11_surface: X11Surface,
    renderer: GlesRenderer,
    damage_tracker: OutputDamageTracker,

    // NOTE: Field drop order matters.
    // On some EGL stacks (notably NVIDIA + egl-wayland), the EGL display drop path may call
    // `eglUnbindWaylandDisplayWL`, which expects the Wayland `wl_display` to still be alive.
    // Keep `display`/`display_handle` *after* the EGL/GLES renderer so they outlive it.
    #[allow(dead_code)]
    display: Rc<RefCell<Display<JwmWaylandState>>>,
    #[allow(dead_code)]
    display_handle: DisplayHandle,

    surfaces_on_output: HashSet<wayland_server::Weak<WlSurface>>,

    cursor_id: Id,
    cursor_size: i32,

    needs_render: bool,
    background_id: Id,

    window_ops: Box<dyn WindowOps>,
    input_ops: Box<dyn InputOps>,
    property_ops: Box<dyn PropertyOps>,
    output_ops: Box<dyn OutputOps>,
    key_ops: Box<dyn KeyOps>,
    cursor_provider: Box<dyn CursorProvider>,
    color_allocator: Box<dyn ColorAllocator>,
}

unsafe impl Send for WaylandX11Backend {}

impl WaylandX11Backend {
    fn request_flush(&self) {
        if !self.flush_pending.swap(true, Ordering::SeqCst) {
            let _ = self.flush_tx.send(());
        }
    }

    fn update_single_output(&mut self, width: i32, height: i32, emit_events: bool) {
        let mut shared = self.shared.lock().unwrap();
        let key: u64 = 0;
        let id = if let Some(id) = shared.output_key_to_id.get(&key).copied() {
            id
        } else {
            let raw = shared.next_output_raw;
            shared.next_output_raw = shared.next_output_raw.wrapping_add(1);
            let id = OutputId(raw);
            shared.output_key_to_id.insert(key, id);
            id
        };

        let info = OutputInfo {
            id,
            name: "x11".into(),
            x: 0,
            y: 0,
            width,
            height,
            scale: 1.0,
            refresh_rate: 60_000,
        };

        if emit_events {
            let mut q = self.pending_events.lock().unwrap();
            if shared.outputs.is_empty() {
                q.push_back(BackendEvent::OutputAdded(info.clone()));
                q.push_back(BackendEvent::ScreenLayoutChanged);
            } else {
                q.push_back(BackendEvent::OutputChanged(info.clone()));
                q.push_back(BackendEvent::ScreenLayoutChanged);
            }
        }

        shared.outputs = vec![info];
    }

    fn render_if_needed(&mut self) -> Result<(), BackendError> {
        if !self.needs_render && !self.state.needs_redraw {
            return Ok(());
        }

        let Some(mode) = self.output.current_mode() else {
            return Ok(());
        };

        let out_w = mode.size.w;
        let out_h = mode.size.h;

        let scale = Scale::from(self.output.current_scale().fractional_scale());
        let ox = 0;
        let oy = 0;
        let output_rect_global = Rectangle::<i32, Logical>::new((ox, oy).into(), (out_w, out_h).into());

        let mut elements: Vec<X11RenderElement<GlesRenderer>> = Vec::new();
        let mut visible_surfaces: HashSet<wayland_server::Weak<WlSurface>> = HashSet::new();
        let mut frame_roots: Vec<WlSurface> = Vec::new();

        // Cursor (top-most). Rendered by us; hide X11 cursor.
        {
            let cursor_pos = self
                .state
                .pointer_location
                .to_physical(scale)
                .to_i32_round::<i32>();
            let cursor_geo: Rectangle<i32, Physical> = Rectangle::new(
                (cursor_pos.x - ox, cursor_pos.y - oy).into(),
                (self.cursor_size, self.cursor_size).into(),
            );
            let cursor = SolidColorRenderElement::new(
                self.cursor_id.clone(),
                cursor_geo,
                0usize,
                Color32F::new(0.95, 0.95, 0.95, 1.0),
                Kind::Cursor,
            );
            elements.push(X11RenderElement::Solid(cursor));
        }

        // Layer surfaces above normal windows.
        {
            let map = layer_map_for_output(&self.output);
            for layer in [WlrLayer::Overlay, WlrLayer::Top] {
                for ls in map.layers_on(layer) {
                    let Some(geo) = map.layer_geometry(ls) else {
                        continue;
                    };
                    let rect_global = Rectangle::<i32, Logical>::new(
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
                            let data = child_states
                                .data_map
                                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>();
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

                    let location: Point<i32, Physical> = (geo.loc.x, geo.loc.y).into();
                    let tree = SurfaceTree::from_surface(&surface);
                    let layer_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                        AsRenderElements::<GlesRenderer>::render_elements(
                            &tree,
                            &mut self.renderer,
                            location,
                            scale,
                            1.0,
                        );
                    elements.extend(layer_elements.into_iter().map(X11RenderElement::Surface));
                }
            }
        }

        for win in self.state.window_stack.iter().rev() {
            if !self.state.mapped_windows.contains(win) {
                continue;
            }
            let Some(geo) = self.state.window_geometry.get(win) else {
                continue;
            };
            let Some(surface) = self.state.surface_for_window(*win) else {
                continue;
            };

            // Render popups belonging to this toplevel above it.
            for (popup_surface, popup_rect) in self.state.popup_rects_for_toplevel(*win) {
                if !popup_rect.overlaps(output_rect_global) {
                    continue;
                }

                frame_roots.push(popup_surface.clone());

                with_surface_tree_downward(
                    &popup_surface,
                    (),
                    |_, _, _| TraversalAction::DoChildren(()),
                    |child_surface, child_states, _| {
                        let data = child_states
                            .data_map
                            .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>();
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

                let location: Point<i32, Physical> =
                    (popup_rect.loc.x - ox, popup_rect.loc.y - oy).into();
                let tree = SurfaceTree::from_surface(&popup_surface);
                let popup_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                    AsRenderElements::<GlesRenderer>::render_elements(
                        &tree,
                        &mut self.renderer,
                        location,
                        scale,
                        1.0,
                    );
                elements.extend(popup_elements.into_iter().map(X11RenderElement::Surface));
            }

            let win_rect = Rectangle::<i32, Logical>::new(
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
                    let data = child_states
                        .data_map
                        .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>();
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

            let location: Point<i32, Physical> = (geo.x - ox, geo.y - oy).into();
            let tree = SurfaceTree::from_surface(&surface);
            let window_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                AsRenderElements::<GlesRenderer>::render_elements(
                    &tree,
                    &mut self.renderer,
                    location,
                    scale,
                    1.0,
                );
            elements.extend(window_elements.into_iter().map(X11RenderElement::Surface));

            // Server-side borders for tiling WM.
            if geo.border > 0 {
                let bw = geo.border as i32;
                let [cr, cg, cb, ca] = self
                    .state
                    .window_border_color
                    .get(&win)
                    .copied()
                    .unwrap_or([0.3, 0.3, 0.35, 1.0]);
                let border_color = Color32F::new(cr, cg, cb, ca);
                let full_geo: Rectangle<i32, Physical> = Rectangle::new(
                    (geo.x - ox - bw, geo.y - oy - bw).into(),
                    (geo.w as i32 + 2 * bw, geo.h as i32 + 2 * bw).into(),
                );
                elements.push(X11RenderElement::Solid(SolidColorRenderElement::new(
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
            let map = layer_map_for_output(&self.output);
            for layer in [WlrLayer::Bottom, WlrLayer::Background] {
                for ls in map.layers_on(layer) {
                    let Some(geo) = map.layer_geometry(ls) else {
                        continue;
                    };
                    let rect_global = Rectangle::<i32, Logical>::new(
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
                            let data = child_states
                                .data_map
                                .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>();
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

                    let location: Point<i32, Physical> = (geo.loc.x, geo.loc.y).into();
                    let tree = SurfaceTree::from_surface(&surface);
                    let layer_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                        AsRenderElements::<GlesRenderer>::render_elements(
                            &tree,
                            &mut self.renderer,
                            location,
                            scale,
                            1.0,
                        );
                    elements.extend(layer_elements.into_iter().map(X11RenderElement::Surface));
                }
            }
        }

        for gone in self.surfaces_on_output.difference(&visible_surfaces) {
            if let Ok(surf) = gone.upgrade() {
                self.output.leave(&surf);
            }
        }
        self.surfaces_on_output = visible_surfaces.clone();

        // Solid background LAST (back-most).
        let bg_geo = Rectangle::<i32, Physical>::from_size((out_w, out_h).into());
        let bg = SolidColorRenderElement::new(
            self.background_id.clone(),
            bg_geo,
            0usize,
            Color32F::new(0.1, 0.15, 0.25, 1.0),
            Kind::Unspecified,
        );
        elements.push(X11RenderElement::Solid(bg));

        let buffer_res = self
            .x11_surface
            .buffer()
            .map_err(|e| BackendError::Other(Box::new(e)));
        let (mut buffer, age) = match buffer_res {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[wayland-x11] failed to acquire X11 buffer: {e:?}");
                self.x11_surface.reset_buffers();
                self.needs_render = false;
                self.state.needs_redraw = false;
                return Ok(());
            }
        };
        let age = age as usize;

        let fb_res = self
            .renderer
            .bind(&mut buffer)
            .map_err(|e| BackendError::Other(Box::new(e)));
        let mut fb = match fb_res {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[wayland-x11] framebuffer bind failed: {e:?}");
                self.x11_surface.reset_buffers();
                self.needs_render = false;
                self.state.needs_redraw = false;
                return Ok(());
            }
        };

        if let Err(e) = self.damage_tracker.render_output(
            &mut self.renderer,
            &mut fb,
            age,
            &elements,
            Color32F::new(0.0, 0.0, 0.0, 1.0),
        ) {
            log::warn!("[wayland-x11] render_output failed: {e:?}");
            drop(fb);
            self.x11_surface.reset_buffers();
            self.needs_render = false;
            self.state.needs_redraw = false;
            return Ok(());
        }

        drop(fb);

        if let Err(err) = self.x11_surface.submit() {
            log::warn!("[wayland-x11] submit failed: {err:?}");
            self.x11_surface.reset_buffers();
            self.needs_render = true;
            return Ok(());
        }

        // Send frame callbacks after a successful submit.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::ZERO);
        let throttle = self
            .output
            .current_mode()
            .and_then(|m| if m.refresh > 0 {
                Some(Duration::from_secs_f64(1_000f64 / m.refresh as f64))
            } else {
                None
            });
        let output = self.output.clone();
        let visible = visible_surfaces;
        for root in &frame_roots {
            send_frames_surface_tree(root, &output, now, throttle, |surface, states| {
                let data = states
                    .data_map
                    .get::<smithay::backend::renderer::utils::RendererSurfaceStateUserData>();
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

        self.needs_render = false;
        self.state.needs_redraw = false;
        self.request_flush();
        Ok(())
    }

    pub fn new() -> Result<Self, BackendError> {
        let event_loop: EventLoop<'static, JwmWaylandState> =
            EventLoop::try_new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let display = Rc::new(RefCell::new(
            Display::new().map_err(|e| BackendError::Other(Box::new(e)))?,
        ));
        let display_handle = display.borrow().handle();

        let (flush_tx, flush_rx) = channel::channel::<()>();
        let flush_pending = Arc::new(AtomicBool::new(false));
        {
            let display = display.clone();
            let flush_pending = flush_pending.clone();
            event_loop
                .handle()
                .insert_source(flush_rx, move |_, _, _state| {
                    if let Err(err) = display.borrow_mut().flush_clients() {
                        log::debug!("wayland flush_clients failed: {err:?}");
                    }
                    flush_pending.store(false, Ordering::SeqCst);
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(wayland flush) failed: {e}")))?;
        }

        let wayland_poll_fd = {
            use std::os::fd::AsFd as _;
            smithay::reexports::rustix::io::dup(display.borrow().as_fd())
                .map_err(|e| BackendError::Other(Box::new(e)))?
        };
        {
            let display = display.clone();
            let flush_tx = flush_tx.clone();
            let flush_pending = flush_pending.clone();
            event_loop
                .handle()
                .insert_source(
                    Generic::new(wayland_poll_fd, Interest::READ, Mode::Level),
                    move |_, _, state| {
                        if let Err(err) = display.borrow_mut().dispatch_clients(state) {
                            log::warn!("wayland dispatch_clients failed: {err:?}");
                        }
                        if !flush_pending.swap(true, Ordering::SeqCst) {
                            let _ = flush_tx.send(());
                        }
                        Ok(PostAction::Continue)
                    },
                )
                .map_err(|e| BackendError::Message(format!("calloop insert_source(wayland display) failed: {e}")))?;
        }

        {
            let initial_configure_timeout = Duration::from_millis(250);
            let tick = Duration::from_millis(50);
            let timer = Timer::from_duration(tick);
            event_loop
                .handle()
                .insert_source(timer, move |_, _, state| {
                    state.ensure_initial_configure_timeout(initial_configure_timeout);
                    TimeoutAction::ToDuration(tick)
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(initial configure timer) failed: {e}")))?;
        }

        let shared = Arc::new(Mutex::new(SharedState::default()));
        let pending_events = Arc::new(Mutex::new(VecDeque::<BackendEvent>::new()));

        // Prepare key binding suppression table (like X11 grabs) for the Wayland path.
        {
            let allowed_mods = Mods::SHIFT
                | Mods::CONTROL
                | Mods::ALT
                | Mods::SUPER
                | Mods::MOD2
                | Mods::MOD3
                | Mods::MOD5;

            let key_bindings = CONFIG
                .load()
                .get_keys()
                .into_iter()
                .map(|k| (k.mask & allowed_mods, k.key_sym))
                .collect::<Vec<_>>();

            let mut s = shared.lock().unwrap();
            s.key_bindings = key_bindings;
        }

        let seat_name = "x11".to_string();
        let (wayland_state, socket_name) = JwmWaylandState::init(
            &display_handle,
            event_loop.handle(),
            pending_events.clone(),
            seat_name,
            true,
        )
        .map_err(|e| BackendError::Message(format!("wayland init failed: {e}")))?;

        if let Some(name) = socket_name.as_deref() {
            unsafe {
                std::env::set_var("WAYLAND_DISPLAY", name);
            }
        }

        let mut state = Box::new(wayland_state);

        let x11_backend = X11Backend::new().map_err(|e| BackendError::Other(Box::new(e)))?;
        let handle = x11_backend.handle();
        let (_node, fd) = handle
            .drm_node()
            .map_err(|e| BackendError::Message(format!("x11 drm_node failed: {e:?}")))?;
        let device = gbm::Device::new(DeviceFd::from(fd))
            .map_err(|e| BackendError::Other(Box::new(e)))?;
        let egl = unsafe { EGLDisplay::new(device.clone()).map_err(|e| BackendError::Other(Box::new(e)))? };
        let context = EGLContext::new(&egl).map_err(|e| BackendError::Other(Box::new(e)))?;
        let mut renderer = unsafe { GlesRenderer::new(context).map_err(|e| BackendError::Other(Box::new(e)))? };
        let _ = renderer.bind_wl_display(&display_handle);

        let window = WindowBuilder::new()
            .title("JWM (wayland-x11)")
            .build(&handle)
            .map_err(|e| BackendError::Other(Box::new(e)))?;

        // Prefer linear/invalid modifiers first for maximum EGLImage compatibility (notably on NVIDIA),
        // but still pass through the full modifier list so allocation doesn't fail on stacks where
        // linear isn't available for the chosen window format.
        //
        // You can override this for troubleshooting:
        // - `JWM_X11_DMABUF_MODIFIERS=invalid` => only implicit/invalid modifier
        // - `JWM_X11_DMABUF_MODIFIERS=linear`  => linear + invalid
        // - `JWM_X11_DMABUF_MODIFIERS=all`     => linear + invalid + all advertised modifiers (default)
        let modifier_policy = std::env::var("JWM_X11_DMABUF_MODIFIERS")
            .unwrap_or_else(|_| "all".to_string())
            .to_lowercase();

        let mut preferred_modifiers = Vec::new();
        match modifier_policy.as_str() {
            "invalid" => {
                preferred_modifiers.push(Modifier::Invalid);
            }
            "linear" => {
                preferred_modifiers.push(Modifier::Linear);
                preferred_modifiers.push(Modifier::Invalid);
            }
            _ => {
                preferred_modifiers.push(Modifier::Linear);
                preferred_modifiers.push(Modifier::Invalid);
                for m in renderer.egl_context().dmabuf_render_formats().iter().map(|f| f.modifier) {
                    if m != Modifier::Linear && m != Modifier::Invalid {
                        preferred_modifiers.push(m);
                    }
                }
            }
        }

        let mut x11_surface = handle
            .create_surface(
                &window,
                DmabufAllocator(GbmAllocator::new(
                    device,
                    GbmBufferFlags::RENDERING,
                )),
                preferred_modifiers.into_iter(),
            )
            .map_err(|e| BackendError::Other(Box::new(e)))?;

        // Preflight: the smithay X11 backend presents dmabufs; we must be able to bind a dmabuf
        // as a framebuffer via EGLImage. If the EGL/GL stack lacks EGLImage support, rendering
        // will fail every frame. Detect this early and provide a clear action.
        {
            let (mut test_buffer, _age) = x11_surface
                .buffer()
                .map_err(|e| BackendError::Message(format!(
                    "[wayland-x11] failed to acquire initial X11 buffer for preflight: {e:?}"
                )))?;
            match renderer.bind(&mut test_buffer) {
                Ok(fb) => {
                    drop(fb);
                    x11_surface.reset_buffers();
                }
                Err(e) => {
                    let egl = renderer.egl_context().display();
                    let egl_ver = egl.get_egl_version();
                    return Err(BackendError::Message(format!(
                        "[wayland-x11] cannot bind dmabuf framebuffer ({e:?}). This typically means your EGL/GL stack cannot render *into* dmabuf via EGLImage on this path. EGL version: {egl_ver:?}.\n\
Troubleshooting (NVIDIA/GBM): try forcing simpler dmabuf modifiers: `JWM_X11_DMABUF_MODIFIERS=invalid` (or `linear`).\n\
Fallback: run the winit backend instead: `JWM_BACKEND=wayland-winit` (same binary)."
                    )));
                }
            }
        }

        let size = window.size();
        let mode = WlMode {
            size: (size.w as i32, size.h as i32).into(),
            refresh: 60_000,
        };

        let output = Output::new(
            "x11".to_string(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "JWM".into(),
                model: "X11".into(),
                serial_number: "Unknown".into(),
            },
        );
        let _global = output.create_global::<JwmWaylandState>(&display_handle);
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
        output.set_preferred(mode);

        state.outputs = vec![output.clone()];
        state.output_rects = vec![smithay::utils::Rectangle::new(
            (0, 0).into(),
            (mode.size.w, mode.size.h).into(),
        )];
        state.needs_redraw = true;

        let exit_requested = Arc::new(AtomicBool::new(false));
        {
            let exit_requested = exit_requested.clone();
            let shared = shared.clone();
            let pending_events = pending_events.clone();
            let flush_tx = flush_tx.clone();
            let flush_pending = flush_pending.clone();
            let output_clone = output.clone();
            event_loop
                .handle()
                .insert_source(x11_backend, move |event, _, state| {
                    match event {
                        X11Event::Focus { focused, .. } => {
                            let debug_keys = std::env::var("JWM_DEBUG_KEYS")
                                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                                .unwrap_or(false);

                            // When the nested X11 window loses focus, we can miss key release
                            // events from the host X server. That can leave modifiers (e.g. Alt)
                            // stuck in the Smithay keyboard state, causing unintended WM
                            // shortcuts like Mod1+s while typing.
                            if !focused {
                                let serial = SCOUNTER.next_serial();
                                if let Some(kbd) = state.seat.get_keyboard() {
                                    if debug_keys {
                                        let pressed = kbd.pressed_keys();
                                        let mods = kbd.modifier_state();
                                        let cached_mods = shared.lock().ok().map(|s| s.mods_state);
                                        log::info!(
                                            "[x11] focus_lost: pressed_keys={} smithay_mods={:?} cached_mods=0x{:x}",
                                            pressed.len(),
                                            mods,
                                            cached_mods.unwrap_or(0)
                                        );
                                    }

                                    // Drop focus so nothing receives synthetic releases.
                                    kbd.set_focus(state, None, serial);

                                    // Release any keys Smithay still considers pressed.
                                    let pressed = kbd.pressed_keys();
                                    for key in pressed {
                                        kbd.input(
                                            state,
                                            key,
                                            smithay::backend::input::KeyState::Released,
                                            serial,
                                            0,
                                            |_, _, _| FilterResult::<()>::Forward,
                                        );
                                    }

                                    // And explicitly reset modifiers.
                                    let _ = kbd.set_modifier_state(ModifiersState::default());
                                }

                                if let Ok(mut s) = shared.lock() {
                                    s.mods_state = 0;
                                    s.suppressed_keycodes.clear();
                                }
                            } else {
                                // On focus regain, clear any leftover suppression bookkeeping.
                                // (We avoid touching modifier state here to not interfere with
                                // keys that might be physically held while refocusing.)
                                if let Ok(mut s) = shared.lock() {
                                    s.suppressed_keycodes.clear();
                                }

                                if debug_keys {
                                    if let Some(kbd) = state.seat.get_keyboard() {
                                        let mods = kbd.modifier_state();
                                        let cached_mods = shared.lock().ok().map(|s| s.mods_state);
                                        log::info!(
                                            "[x11] focus_gained: smithay_mods={:?} cached_mods=0x{:x}",
                                            mods,
                                            cached_mods.unwrap_or(0)
                                        );
                                    }
                                }
                            }
                        }
                        X11Event::CloseRequested { .. } => {
                            exit_requested.store(true, Ordering::SeqCst);
                        }
                        X11Event::Resized { new_size, .. } => {
                            let mode = WlMode {
                                size: (new_size.w as i32, new_size.h as i32).into(),
                                refresh: 60_000,
                            };
                            if let Some(old) = output_clone.current_mode() {
                                output_clone.delete_mode(old);
                            }
                            output_clone.change_current_state(Some(mode), None, None, Some((0, 0).into()));
                            output_clone.set_preferred(mode);
                            state.output_rects = vec![smithay::utils::Rectangle::new(
                                (0, 0).into(),
                                (mode.size.w, mode.size.h).into(),
                            )];
                            state.needs_redraw = true;

                            let id = {
                                let mut s = shared.lock().unwrap();
                                if let Some(id) = s.output_key_to_id.get(&0).copied() {
                                    id
                                } else {
                                    let raw = s.next_output_raw;
                                    s.next_output_raw = s.next_output_raw.wrapping_add(1);
                                    let id = OutputId(raw);
                                    s.output_key_to_id.insert(0, id);
                                    id
                                }
                            };
                            let info = OutputInfo {
                                id,
                                name: "x11".into(),
                                x: 0,
                                y: 0,
                                width: mode.size.w,
                                height: mode.size.h,
                                scale: 1.0,
                                refresh_rate: 60_000,
                            };
                            {
                                let mut s = shared.lock().unwrap();
                                s.outputs = vec![info.clone()];
                            }
                            let mut q = pending_events.lock().unwrap();
                            q.push_back(BackendEvent::OutputChanged(info));
                            q.push_back(BackendEvent::ScreenLayoutChanged);
                        }
                        X11Event::Refresh { .. } | X11Event::PresentCompleted { .. } => {
                            state.needs_redraw = true;
                        }
                        X11Event::Input { event, .. } => {
                            process_input_event_windowed(
                                event,
                                state,
                                &shared,
                                &pending_events,
                            );

                            // Input events can enqueue Wayland protocol messages; flush them promptly.
                            if !flush_pending.swap(true, Ordering::SeqCst) {
                                let _ = flush_tx.send(());
                            }
                        }
                    }
                    ()
                })
                .map_err(|e| BackendError::Message(format!("calloop insert_source(x11) failed: {e}")))?;
        }

        let mut backend = Self {
            display,
            display_handle,
            event_loop: SendWrapper(event_loop),
            state,
            socket_name,
            pending_events,
            flush_tx,
            flush_pending,
            shared,
            exit_requested,
            output: output.clone(),
            window,
            x11_surface,
            renderer,
            damage_tracker: OutputDamageTracker::from_output(&output),

            surfaces_on_output: HashSet::new(),

            cursor_id: Id::new(),
            cursor_size: 16,
            needs_render: true,
            background_id: Id::new(),
            window_ops: Box::new(wayland_dummy_ops::DummyWindowOps),
            input_ops: Box::new(wayland_dummy_ops::DummyInputOps),
            property_ops: Box::new(wayland_dummy_ops::DummyPropertyOps),
            output_ops: Box::new(wayland_dummy_ops::DummyOutputOps),
            key_ops: Box::new(wayland_dummy_ops::DummyKeyOps),
            cursor_provider: Box::new(wayland_dummy_ops::DummyCursorProvider),
            color_allocator: Box::new(wayland_dummy_ops::DummyColorAllocator),
        };

        backend.update_single_output(mode.size.w, mode.size.h, true);
        backend.output_ops = Box::new(WaylandOutputOps {
            shared: backend.shared.clone(),
        });

        backend.key_ops = Box::new(wayland_key_ops::UdevKeyOps::new()?);

        let state_ptr: *mut JwmWaylandState = &mut *backend.state;
        backend.window_ops = Box::new(WaylandWindowOps {
            state: SendWrapper(state_ptr),
            flush_tx: backend.flush_tx.clone(),
            flush_pending: backend.flush_pending.clone(),
        });

        backend.property_ops = Box::new(WaylandPropertyOps {
            state: SendWrapper(state_ptr),
            flush_tx: backend.flush_tx.clone(),
            flush_pending: backend.flush_pending.clone(),
        });

        // We render our own cursor; hide X11 cursor.
        backend.window.set_cursor_visible(false);

        Ok(backend)
    }
}

fn process_input_event_windowed<B: InputBackend>(
    event: InputEvent<B>,
    state: &mut JwmWaylandState,
    shared: &Arc<Mutex<SharedState>>,
    pending_events: &Arc<Mutex<VecDeque<BackendEvent>>>,
) {
    match event {
        InputEvent::PointerMotion { event, .. } => {
            let delta = event.delta();
            let time = event.time_msec();
            let (x, y, output) = {
                let mut s = shared.lock().unwrap();
                s.pointer_x += delta.x;
                s.pointer_y += delta.y;
                let x = s.pointer_x;
                let y = s.pointer_y;
                let output = s
                    .outputs
                    .iter()
                    .find(|o| {
                        (x as i32) >= o.x
                            && (y as i32) >= o.y
                            && (x as i32) < (o.x + o.width)
                            && (y as i32) < (o.y + o.height)
                    })
                    .map(|o| o.id);
                (x, y, output)
            };

            let location: Point<f64, Logical> = (x, y).into();
            state.pointer_location = location;
            state.needs_redraw = true;

            // If a popup grab is active, leaving the grab area dismisses the popups.
            if let Some(grab_win) = state.popup_grab_toplevel {
                if let Some(area) = state.popup_grab_area(grab_win) {
                    let px = location.x.round() as i32;
                    let py = location.y.round() as i32;
                    let inside = px >= area.loc.x
                        && py >= area.loc.y
                        && px < area.loc.x + area.size.w
                        && py < area.loc.y + area.size.h;
                    if !inside {
                        state.dismiss_popups_for_toplevel(grab_win);
                        state.needs_redraw = true;
                    }
                }
            }

            let under = state.surface_under(location);
            let hit = under
                .as_ref()
                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
            let focus = under.map(|(_win, surface, origin)| (surface, origin));

            if let Some(pointer) = state.seat.get_pointer() {
                pointer.motion(
                    state,
                    focus,
                    &MotionEvent {
                        location,
                        serial: SCOUNTER.next_serial(),
                        time,
                    },
                );
                pointer.frame(state);
            }

            pending_events
                .lock()
                .unwrap()
                .push_back(BackendEvent::MotionNotify {
                    target: hit.unwrap_or(HitTarget::Background { output }),
                    root_x: x,
                    root_y: y,
                    time,
                });
        }

        InputEvent::PointerMotionAbsolute { event, .. } => {
            let time = event.time_msec();
            let (x, y, output) = {
                let mut s = shared.lock().unwrap();
                let (w, h, origin_x, origin_y, output) = if let Some(first) = s.outputs.first() {
                    (
                        first.width.max(1) as i32,
                        first.height.max(1) as i32,
                        first.x,
                        first.y,
                        Some(first.id),
                    )
                } else {
                    (1280, 720, 0, 0, None)
                };
                let pos = event.position_transformed(smithay::utils::Size::from((w, h)));
                s.pointer_x = origin_x as f64 + pos.x;
                s.pointer_y = origin_y as f64 + pos.y;
                (s.pointer_x, s.pointer_y, output)
            };

            let location: Point<f64, Logical> = (x, y).into();
            state.pointer_location = location;
            state.needs_redraw = true;

            if let Some(grab_win) = state.popup_grab_toplevel {
                if let Some(area) = state.popup_grab_area(grab_win) {
                    let px = location.x.round() as i32;
                    let py = location.y.round() as i32;
                    let inside = px >= area.loc.x
                        && py >= area.loc.y
                        && px < area.loc.x + area.size.w
                        && py < area.loc.y + area.size.h;
                    if !inside {
                        state.dismiss_popups_for_toplevel(grab_win);
                        state.needs_redraw = true;
                    }
                }
            }

            let under = state.surface_under(location);
            let hit = under
                .as_ref()
                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
            let focus = under.map(|(_win, surface, origin)| (surface, origin));

            if let Some(pointer) = state.seat.get_pointer() {
                pointer.motion(
                    state,
                    focus,
                    &MotionEvent {
                        location,
                        serial: SCOUNTER.next_serial(),
                        time,
                    },
                );
                pointer.frame(state);
            }

            pending_events
                .lock()
                .unwrap()
                .push_back(BackendEvent::MotionNotify {
                    target: hit.unwrap_or(HitTarget::Background { output }),
                    root_x: x,
                    root_y: y,
                    time,
                });
        }

        InputEvent::PointerButton { event, .. } => {
            let time = event.time_msec();
            let button_code = event.button_code();
            let pressed = matches!(
                event.state(),
                smithay::backend::input::ButtonState::Pressed
            );

            let (x, y, output, mods_state) = {
                let s = shared.lock().unwrap();
                let x = s.pointer_x;
                let y = s.pointer_y;
                let output = s
                    .outputs
                    .iter()
                    .find(|o| {
                        (x as i32) >= o.x
                            && (y as i32) >= o.y
                            && (x as i32) < (o.x + o.width)
                            && (y as i32) < (o.y + o.height)
                    })
                    .map(|o| o.id);
                (x, y, output, s.mods_state)
            };

            let location: Point<f64, Logical> = (x, y).into();

            // Minimal xdg_popup grab behavior: click outside dismisses.
            if pressed {
                if let Some(grab_win) = state.popup_grab_toplevel {
                    let in_any_popup = state
                        .popup_rects_for_toplevel(grab_win)
                        .iter()
                        .any(|(_surf, rect)| {
                            let x0 = rect.loc.x as f64;
                            let y0 = rect.loc.y as f64;
                            let x1 = x0 + rect.size.w as f64;
                            let y1 = y0 + rect.size.h as f64;
                            location.x >= x0 && location.y >= y0 && location.x < x1 && location.y < y1
                        });

                    if !in_any_popup {
                        state.dismiss_popups_for_toplevel(grab_win);
                        state.needs_redraw = true;
                    }
                }
            }

            let under = state.surface_under(location);
            let hit = under
                .as_ref()
                .and_then(|(win, _, _)| win.map(HitTarget::Surface));
            let focus = under.map(|(_win, surface, origin)| (surface, origin));

            if let Some(pointer) = state.seat.get_pointer() {
                // Ensure focus is up-to-date before sending the button.
                pointer.motion(
                    state,
                    focus,
                    &MotionEvent {
                        location,
                        serial: SCOUNTER.next_serial(),
                        time,
                    },
                );

                pointer.button(
                    state,
                    &ButtonEvent {
                        serial: SCOUNTER.next_serial(),
                        time,
                        button: button_code,
                        state: if pressed {
                            smithay::backend::input::ButtonState::Pressed
                        } else {
                            smithay::backend::input::ButtonState::Released
                        },
                    },
                );
                pointer.frame(state);
            }

            // Focus follows click: best-effort.
            if pressed {
                if let Some(kbd) = state.seat.get_keyboard() {
                    if let Some((_win, surface, _origin)) = state.surface_under(location) {
                        let layer_interactivity = state
                            .layer_shell_state
                            .layer_surfaces()
                            .find(|l| l.wl_surface().id() == surface.id())
                            .map(|l| l.with_cached_state(|d| d.keyboard_interactivity));

                        let should_focus = match layer_interactivity {
                            Some(KeyboardInteractivity::None) => false,
                            Some(KeyboardInteractivity::OnDemand) => true,
                            Some(KeyboardInteractivity::Exclusive) => true,
                            None => true,
                        };

                        if should_focus {
                            let win = state.surface_to_window.get(&surface.id()).copied();
                            kbd.set_focus(state, Some(surface.clone()), SCOUNTER.next_serial());
                            state.set_active_toplevel(win);
                        }
                    }
                }
            }

            // JWM expects X11-like button numbers for shortcuts.
            let detail_btn: u8 = u8::try_from(button_code & 0xFF).unwrap_or(0);

            if pressed {
                pending_events
                    .lock()
                    .unwrap()
                    .push_back(BackendEvent::ButtonPress {
                        target: hit.unwrap_or(HitTarget::Background { output }),
                        state: mods_state,
                        detail: detail_btn,
                        time,
                        root_x: x,
                        root_y: y,
                    });
            } else {
                pending_events
                    .lock()
                    .unwrap()
                    .push_back(BackendEvent::ButtonRelease {
                        target: hit.unwrap_or(HitTarget::Background { output }),
                        time,
                    });
            }
        }

        InputEvent::Keyboard { event, .. } => {
            let time = InputEventExt::time_msec(&event);
            let keycode = event.key_code();
            let state_key = event.state();
            let serial = SCOUNTER.next_serial();
            let pressed = matches!(state_key, smithay::backend::input::KeyState::Pressed);

            // If nothing is focused, focus the surface under the pointer (best-effort).
            if let Some(kbd) = state.seat.get_keyboard() {
                if kbd.current_focus().is_none() {
                    let under = state.surface_under(state.pointer_location);
                    if let Some((_win, surface, _origin)) = under {
                        kbd.set_focus(state, Some(surface), serial);
                    }
                }

                // Route keyboard to exclusive layer-shell surfaces if any.
                let cfg = crate::config::CONFIG.load();
                let bar_name = cfg.status_bar_name();
                let exclusive_surface = state
                    .layer_shell_state
                    .layer_surfaces()
                    .rev()
                    .find_map(|layer| {
                        if !bar_name.is_empty() {
                            if let Some(win) = state
                                .surface_to_window
                                .get(&layer.wl_surface().id())
                                .copied()
                            {
                                let title = state
                                    .window_title
                                    .get(&win)
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let app_id = state
                                    .window_app_id
                                    .get(&win)
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                if title == bar_name || app_id == bar_name {
                                    return None;
                                }
                            }
                        }

                        let exclusive = layer.with_cached_state(|data| {
                            let exclusive_zone: i32 = data.exclusive_zone.into();
                            data.keyboard_interactivity == KeyboardInteractivity::Exclusive
                                && (data.layer == WlrLayer::Top || data.layer == WlrLayer::Overlay)
                                && exclusive_zone != 0
                        });
                        if exclusive {
                            Some(layer.wl_surface().clone())
                        } else {
                            None
                        }
                    });

                if let Some(surface) = exclusive_surface {
                    kbd.set_focus(state, Some(surface), serial);
                }

                // Build key suppression against WM shortcuts.
                kbd.input(
                    state,
                    keycode,
                    state_key,
                    serial,
                    time,
                    |_, modifiers, keysym_handle| {
                        let keycode_u32 = u32::from(keycode);
                        let xkb_keycode_u8 = u8::try_from(keycode_u32).unwrap_or(0);
                        let keysym = keysym_handle.modified_sym().raw();

                        // Keep modifier state in sync.
                        let mods_bits = mods_from_smithay(modifiers).bits();
                        if let Some(mut s) = shared.lock().ok() {
                            s.mods_state = mods_bits;
                        }

                        // Only intercept on press; releases for suppressed keys are also intercepted.
                        if !pressed {
                            if let Ok(mut s) = shared.lock() {
                                if s.suppressed_keycodes.remove(&xkb_keycode_u8) {
                                    return FilterResult::Intercept(());
                                }
                            }
                            return FilterResult::Forward;
                        }

                        let allowed_mods = Mods::SHIFT
                            | Mods::CONTROL
                            | Mods::ALT
                            | Mods::SUPER
                            | Mods::MOD2
                            | Mods::MOD3
                            | Mods::MOD5;
                        let clean_mods = mods_from_smithay(modifiers) & allowed_mods;

                        let should_suppress = if let Ok(mut s) = shared.lock() {
                            // Cache base keysym (unmodified) for better shortcut matching.
                            if (xkb_keycode_u8 as usize) < s.keysym_table.len() {
                                if s.keysym_table[xkb_keycode_u8 as usize] == 0 {
                                    let base = keysym_handle
                                        .raw_latin_sym_or_raw_current_sym()
                                        .unwrap_or_else(|| keysym_handle.modified_sym());
                                    s.keysym_table[xkb_keycode_u8 as usize] = base.raw();
                                }
                            }

                            s.key_bindings
                                .iter()
                                .any(|(m, ks)| *ks == keysym && *m == clean_mods)
                        } else {
                            false
                        };

                        if should_suppress {
                            if let Ok(mut s) = shared.lock() {
                                s.suppressed_keycodes.insert(xkb_keycode_u8);
                            }
                            FilterResult::Intercept(())
                        } else {
                            FilterResult::Forward
                        }
                    },
                );

                // JWM only uses press for shortcuts.
                if pressed {
                    let keycode_u32 = u32::from(keycode);
                    let keycode_u8 = u8::try_from(keycode_u32).unwrap_or(0);
                    let mods_state = shared.lock().unwrap().mods_state;
                    pending_events
                        .lock()
                        .unwrap()
                        .push_back(BackendEvent::KeyPress {
                            keycode: keycode_u8,
                            state: mods_state,
                            time,
                        });
                }
            }
        }

        _ => {}
    }
}

impl Backend for WaylandX11Backend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            can_warp_pointer: false,
            supports_client_list: false,
        }
    }

    fn root_window(&self) -> Option<WindowId> {
        Some(WindowId::from_raw(0))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn check_existing_wm(&self) -> Result<(), BackendError> {
        Ok(())
    }

    fn window_ops(&self) -> &dyn WindowOps {
        &*self.window_ops
    }

    fn input_ops(&self) -> &dyn InputOps {
        &*self.input_ops
    }

    fn property_ops(&self) -> &dyn PropertyOps {
        &*self.property_ops
    }

    fn output_ops(&self) -> &dyn OutputOps {
        &*self.output_ops
    }

    fn key_ops(&self) -> &dyn KeyOps {
        &*self.key_ops
    }

    fn key_ops_mut(&mut self) -> &mut dyn KeyOps {
        &mut *self.key_ops
    }

    fn cursor_provider(&mut self) -> &mut dyn CursorProvider {
        &mut *self.cursor_provider
    }

    fn color_allocator(&mut self) -> &mut dyn ColorAllocator {
        &mut *self.color_allocator
    }

    fn on_focused_client_changed(&mut self, win: Option<WindowId>) -> Result<(), BackendError> {
        match win {
            Some(w) => self.window_ops.set_input_focus(w)?,
            None => self.window_ops.set_input_focus_root()?,
        }
        self.state.set_active_toplevel(win);
        self.state.needs_redraw = true;
        self.needs_render = true;
        self.request_flush();
        Ok(())
    }

    fn begin_move(&mut self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn begin_resize(&mut self, _win: WindowId, _edge: ResizeEdge) -> Result<(), BackendError> {
        Ok(())
    }

    fn handle_motion(&mut self, _x: f64, _y: f64, _time: u32) -> Result<bool, BackendError> {
        Ok(false)
    }

    fn handle_button_release(&mut self, _time: u32) -> Result<bool, BackendError> {
        Ok(false)
    }

    fn request_render(&mut self) {
        self.needs_render = true;
        self.request_flush();
    }

    fn run(&mut self, handler: &mut dyn EventHandler) -> Result<(), BackendError> {
        loop {
            while let Some(ev) = { self.pending_events.lock().unwrap().pop_front() } {
                handler.handle_event(self, ev)?;
            }

            handler.update(self)?;
            self.render_if_needed()?;

            if handler.should_exit() || self.exit_requested.load(Ordering::SeqCst) {
                break;
            }

            self.event_loop
                .dispatch(Some(Duration::from_millis(16)), &mut *self.state)
                .map_err(|e| BackendError::Other(Box::new(e)))?;
        }
        Ok(())
    }
}
