// src/backend/wayland/grabs.rs
use smithay::{
    desktop::{space::SpaceElement, Window},
    input::pointer::{
        AxisFrame, ButtonEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

use super::event_source::JwmWlState;
use smithay::input::{
    pointer::{
        GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent,
        GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
        GestureSwipeUpdateEvent,
    },
    SeatHandler,
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
