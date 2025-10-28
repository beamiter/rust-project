// src/backend/wayland/grabs.rs
use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel, //
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
    wayland::compositor::with_states,
    wayland::shell::xdg::SurfaceCachedState,
};

use super::event_source::JwmWlState;
use smithay::input::pointer::{
    GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
    GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent,
};

pub struct PointerMoveSurfaceGrab {
    pub start_data: PointerGrabStartData<JwmWlState>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
}

impl PointerGrab<JwmWlState> for PointerMoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        // 注意：这里我们直接使用 WlSurface，因为你的 SeatHandler 是这样定义的
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);

        let delta = event.location - self.start_data.location;
        let new_location = self.initial_window_location.to_f64() + delta;

        data.space.lock().unwrap().map_element(
            self.window.clone(),
            new_location.to_i32_round(),
            true,
        );
    }

    fn button(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn axis(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details)
    }

    fn frame(&mut self, data: &mut JwmWlState, handle: &mut PointerInnerHandle<'_, JwmWlState>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }

    fn start_data(&self) -> &PointerGrabStartData<JwmWlState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut JwmWlState) {}
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ResizeEdge: u32 {
        const NONE = 0;
        const TOP = 1;
        const BOTTOM = 2;
        const LEFT = 4;
        const TOP_LEFT = 5;
        const BOTTOM_LEFT = 6;
        const RIGHT = 8;
        const TOP_RIGHT = 9;
        const BOTTOM_RIGHT = 10;
    }
}

pub struct PointerResizeSurfaceGrab {
    pub start_data: PointerGrabStartData<JwmWlState>,
    pub window: Window,
    pub edges: ResizeEdge,
    pub initial_window_location: Point<i32, Logical>,
    pub initial_window_size: smithay::utils::Size<i32, Logical>,
    pub last_window_size: smithay::utils::Size<i32, Logical>,
}

impl PointerGrab<JwmWlState> for PointerResizeSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);

        let (mut dx, mut dy) = (event.location - self.start_data.location).into();

        let mut new_window_width = self.initial_window_size.w;
        let mut new_window_height = self.initial_window_size.h;

        if self.edges.contains(ResizeEdge::LEFT) {
            dx = -dx;
            new_window_width = (self.initial_window_size.w as f64 + dx) as i32;
        } else if self.edges.contains(ResizeEdge::RIGHT) {
            new_window_width = (self.initial_window_size.w as f64 + dx) as i32;
        }

        if self.edges.contains(ResizeEdge::TOP) {
            dy = -dy;
            new_window_height = (self.initial_window_size.h as f64 + dy) as i32;
        } else if self.edges.contains(ResizeEdge::BOTTOM) {
            new_window_height = (self.initial_window_size.h as f64 + dy) as i32;
        }

        let (min_size, max_size) =
            with_states(self.window.toplevel().unwrap().wl_surface(), |states| {
                let mut cached = states.cached_state.get::<SurfaceCachedState>();
                let state_data = cached.current(); // 调用 .current() 获取实际状态
                (state_data.min_size, state_data.max_size)
            });

        let min_width = min_size.w.max(1);
        let min_height = min_size.h.max(1);
        let max_width = if max_size.w == 0 {
            i32::MAX
        } else {
            max_size.w
        };
        let max_height = if max_size.h == 0 {
            i32::MAX
        } else {
            max_size.h
        };

        new_window_width = new_window_width.max(min_width).min(max_width);
        new_window_height = new_window_height.max(min_height).min(max_height);

        self.last_window_size = (new_window_width, new_window_height).into();

        let toplevel = self.window.toplevel().unwrap();
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
            state.size = Some(self.last_window_size);
        });
        toplevel.send_pending_configure();
    }

    fn button(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, data, event.serial, event.time, true);

            let toplevel = self.window.toplevel().unwrap();
            toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Resizing);
                state.size = Some(self.last_window_size);
            });
            toplevel.send_pending_configure();

            // 如果是从左边或上边调整大小，需要移动窗口
            if self.edges.intersects(ResizeEdge::TOP_LEFT) {
                let mut location = data
                    .space
                    .lock()
                    .unwrap()
                    .element_location(&self.window)
                    .unwrap();
                let geometry = self.window.geometry();

                if self.edges.contains(ResizeEdge::LEFT) {
                    location.x = self.initial_window_location.x
                        + (self.initial_window_size.w - geometry.size.w);
                }
                if self.edges.contains(ResizeEdge::TOP) {
                    location.y = self.initial_window_location.y
                        + (self.initial_window_size.h - geometry.size.h);
                }
                data.space
                    .lock()
                    .unwrap()
                    .map_element(self.window.clone(), location, true);
            }
        }
    }

    fn start_data(&self) -> &PointerGrabStartData<JwmWlState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut JwmWlState) {}

    // 其他 Grab 方法保持和 PointerMoveSurfaceGrab 一样，转发给 handle 即可
    fn relative_motion(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }
    fn axis(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details)
    }
    fn frame(&mut self, data: &mut JwmWlState, handle: &mut PointerInnerHandle<'_, JwmWlState>) {
        handle.frame(data);
    }
    fn gesture_swipe_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }
    fn gesture_swipe_update(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }
    fn gesture_swipe_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }
    fn gesture_pinch_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }
    fn gesture_pinch_update(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }
    fn gesture_pinch_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }
    fn gesture_hold_begin(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }
    fn gesture_hold_end(
        &mut self,
        data: &mut JwmWlState,
        handle: &mut PointerInnerHandle<'_, JwmWlState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }
}
