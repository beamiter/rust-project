// src/core/controller.rs
use crate::backend::api::HitTarget;

use crate::backend::api::{
    Backend, NetWmAction, NetWmState, OutputInfo, PropertyKind, WindowChanges,
};
use crate::backend::common_define::{KeySym, Mods, WindowId};

pub trait WMController {
    // === 硬件与输出 ===
    fn on_output_added(&mut self, backend: &mut dyn Backend, info: OutputInfo);
    fn on_output_removed(
        &mut self,
        backend: &mut dyn Backend,
        id: crate::backend::common_define::OutputId,
    );
    fn on_output_changed(&mut self, backend: &mut dyn Backend, info: OutputInfo);
    fn on_screen_layout_changed(&mut self, backend: &mut dyn Backend);
    fn on_child_process_exited(&mut self, backend: &mut dyn Backend);

    // === 窗口生命周期 ===
    fn on_map_request(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_unmap_notify(&mut self, backend: &mut dyn Backend, win: WindowId, from_configure: bool);
    fn on_destroy_notify(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_window_configured(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    );
    fn on_mapping_notify(&mut self, backend: &mut dyn Backend);

    // === 输入事件 ===

    fn on_button_press(
        &mut self,
        backend: &mut dyn Backend,
        target: HitTarget,
        state: u16,
        detail: u8,
        time: u32,
    );
    fn on_motion_notify(
        &mut self,
        backend: &mut dyn Backend,
        target: HitTarget,
        root_x: f64,
        root_y: f64,
        time: u32,
    );
    fn on_button_release(&mut self, backend: &mut dyn Backend, target: HitTarget, time: u32);
    fn on_key_press(&mut self, backend: &mut dyn Backend, keycode: u8, mods: u16, time: u32);
    fn on_enter_notify(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        root_x: f64,
        root_y: f64,
        mode: crate::backend::api::NotifyMode,
    );
    fn on_leave_notify(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_focus_in(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_focus_out(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_expose(&mut self, backend: &mut dyn Backend, win: WindowId);

    // === 客户端请求 / 协议 ===
    fn on_configure_request(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        mask_bits: u16,
        changes: WindowChanges,
    );
    fn on_property_changed(&mut self, backend: &mut dyn Backend, win: WindowId, kind: PropertyKind);
    fn on_client_message(&mut self, backend: &mut dyn Backend, win: WindowId); // For ActiveWindowMessage
    fn on_window_state_request(
        &mut self,
        backend: &mut dyn Backend,
        win: WindowId,
        action: NetWmAction,
        state: NetWmState,
    );
    fn on_wm_keyboard_shortcut(&mut self, backend: &mut dyn Backend, keysym: KeySym, mods: Mods);
}
