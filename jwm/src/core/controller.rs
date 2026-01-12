// src/core/controller.rs

use crate::backend::api::{Backend, Geometry};
use crate::backend::common_define::{KeySym, Mods, WindowId};

/// 这是一个核心 trait，Jwm 实现它。
/// 后端（X11 Event Loop 或 Smithay Input Handler）调用这些方法。
pub trait WMController {
    // 窗口生命周期
    fn on_map_request(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_unmap_notify(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_destroy_notify(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_configure_notify(&mut self, backend: &mut dyn Backend, win: WindowId, geom: Geometry);

    // 输入事件
    fn on_key_press(&mut self, backend: &mut dyn Backend, mods: Mods, key: KeySym);
    fn on_enter_notify(&mut self, backend: &mut dyn Backend, win: WindowId);
    fn on_focus_in(&mut self, backend: &mut dyn Backend, win: WindowId);

    // 输出变化
    fn on_screen_layout_change(&mut self, backend: &mut dyn Backend);
}
