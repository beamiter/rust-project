// src/backend/wayland/input_ops.rs
use crate::backend::api::{AllowMode, InputOps, WindowId};
use std::sync::{Arc, Mutex};

use crate::backend::wayland::event_source::JwmWlState;
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
    PointerInnerHandle, RelativeMotionEvent,
};
use smithay::input::SeatHandler;
use smithay::utils::{Logical, Point};

#[derive(Clone)]
pub struct PointerController;
impl PointerController {
    pub fn new() -> Self {
        Self
    }
    pub fn new_dummy() -> Self {
        Self
    }
}

pub struct WaylandInputOps {
    _ctrl: PointerController,
}
impl WaylandInputOps {
    pub fn new(ctrl: PointerController) -> Self {
        Self { _ctrl: ctrl }
    }
}

// 完整 PointerGrab 实现，参考 anvil 的 grabs，全部转发到 handle
struct DragGrab<F>
where
    F: FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>> + Send + 'static,
{
    start_data: PointerGrabStartData<super::event_source::JwmWlState>,
    on_motion: Arc<Mutex<F>>,
}

impl<F> PointerGrab<super::event_source::JwmWlState> for DragGrab<F>
where
    F: FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>> + Send + 'static,
{
    fn motion(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        _focus: Option<(
            <JwmWlState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &MotionEvent,
    ) {
        let pos = event.location;
        if let Ok(mut cb) = self.on_motion.lock() {
            let _ = (cb)(pos.x as i16, pos.y as i16, event.time);
        }
        handle.motion(data, None, event);
    }

    fn relative_motion(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        focus: Option<(
            <JwmWlState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        frame: AxisFrame,
    ) {
        handle.axis(data, frame)
    }

    fn frame(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
    ) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut super::event_source::JwmWlState,
        handle: &mut PointerInnerHandle<'_, super::event_source::JwmWlState>,
        event: &smithay::input::pointer::GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }

    fn start_data(&self) -> &PointerGrabStartData<super::event_source::JwmWlState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut super::event_source::JwmWlState) {}
}

impl InputOps for WaylandInputOps {
    fn grab_pointer(
        &self,
        _mask: u32,
        _cursor: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }
    fn ungrab_pointer(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn allow_events(&self, _mode: AllowMode, _time: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), Box<dyn std::error::Error>> {
        Ok((0, 0, 0, 0))
    }
    fn warp_pointer_to_window(
        &self,
        _win: WindowId,
        _x: i16,
        _y: i16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("Wayland does not support pointer warping".into())
    }
    fn drag_loop(
        &self,
        _cursor: Option<u64>,
        _warp_to: Option<(i16, i16)>,
        _target: WindowId,
        _on_motion: &mut dyn FnMut(i16, i16, u32) -> Result<(), Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 正确做法是：像 anvil 一样在状态线程里 pointer.set_grab(self, DragGrab{...}, serial, Focus::Clear)
        // InputOps 这里缺少 &mut JwmWlState 的上下文，先返回 Ok(())，后续接 winit/udev 后再调度 set_grab。
        Ok(())
    }
}
