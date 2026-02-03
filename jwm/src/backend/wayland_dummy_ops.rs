use crate::backend::api::*;
use crate::backend::common_define::*;
use crate::backend::error::BackendError;
// 移除了 use std::any::Any;

// ------------------------------------------------------------------
// 空的 WindowOps
// ------------------------------------------------------------------
pub struct DummyWindowOps;
impl WindowOps for DummyWindowOps {
    // ... 代码内容保持不变，只是为了去除 warning ...
    fn set_position(&self, _win: WindowId, _x: i32, _y: i32) -> Result<(), BackendError> {
        Ok(())
    }
    fn configure(
        &self,
        _win: WindowId,
        _x: i32,
        _y: i32,
        _w: u32,
        _h: u32,
        _border: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_decoration_style(
        &self,
        _win: WindowId,
        _border_width: u32,
        _border_color: Pixel,
    ) -> Result<(), BackendError> {
        Ok(())
    }
    fn raise_window(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn map_window(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn unmap_window(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn close_window(&self, _win: WindowId) -> Result<CloseResult, BackendError> {
        Ok(CloseResult::Graceful)
    }
    fn set_input_focus(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn set_input_focus_root(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn get_window_attributes(&self, _win: WindowId) -> Result<WindowAttributes, BackendError> {
        Ok(WindowAttributes {
            override_redirect: false,
            map_state_viewable: true,
        })
    }
    fn get_geometry(&self, _win: WindowId) -> Result<Geometry, BackendError> {
        Ok(Geometry::default())
    }
    fn scan_windows(&self) -> Result<Vec<WindowId>, BackendError> {
        Ok(vec![])
    }
    fn flush(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn kill_client(&self, _win: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn apply_window_changes(
        &self,
        _win: WindowId,
        _changes: WindowChanges,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

// ... InputOps, PropertyOps, OutputOps, KeyOps, CursorProvider, ColorAllocator 保持不变
// 只要确保没有引用 std::any::Any 即可
pub struct DummyInputOps;
impl InputOps for DummyInputOps {
    fn set_cursor(&self, _kind: StdCursorKind) -> Result<(), BackendError> {
        Ok(())
    }
    fn get_pointer_position(&self) -> Result<(f64, f64), BackendError> {
        Ok((0.0, 0.0))
    }
    fn grab_pointer(&self, _mask: u32, _cursor: Option<u64>) -> Result<bool, BackendError> {
        Ok(true)
    }
    fn ungrab_pointer(&self) -> Result<(), BackendError> {
        Ok(())
    }
    fn query_pointer_root(&self) -> Result<(i32, i32, u16, u16), BackendError> {
        Ok((0, 0, 0, 0))
    }
}

pub struct DummyPropertyOps;
impl PropertyOps for DummyPropertyOps {
    fn get_title(&self, _win: WindowId) -> String {
        "Wayland Window".to_string()
    }
    fn get_class(&self, _win: WindowId) -> (String, String) {
        ("app".into(), "App".into())
    }
    fn get_window_types(&self, _win: WindowId) -> Vec<WindowType> {
        vec![WindowType::Normal]
    }
    fn is_fullscreen(&self, _win: WindowId) -> bool {
        false
    }
    fn set_fullscreen_state(&self, _win: WindowId, _on: bool) -> Result<(), BackendError> {
        Ok(())
    }
    fn transient_for(&self, _win: WindowId) -> Option<WindowId> {
        None
    }
    fn get_wm_hints(&self, _win: WindowId) -> Option<WmHints> {
        None
    }
    fn set_urgent_hint(&self, _win: WindowId, _urgent: bool) -> Result<(), BackendError> {
        Ok(())
    }
    fn fetch_normal_hints(&self, _win: WindowId) -> Result<Option<NormalHints>, BackendError> {
        Ok(None)
    }
    fn set_window_strut_top(
        &self,
        _win: WindowId,
        _top: u32,
        _sx: u32,
        _ex: u32,
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
    fn set_client_info_props(
        &self,
        _win: WindowId,
        _tags: u32,
        _monitor_num: u32,
    ) -> Result<(), BackendError> {
        Ok(())
    }
}

pub struct DummyOutputOps;
impl OutputOps for DummyOutputOps {
    fn enumerate_outputs(&self) -> Vec<OutputInfo> {
        vec![OutputInfo {
            id: crate::backend::common_define::OutputId(0),
            name: "Virtual-1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            refresh_rate: 60000,
        }]
    }
    fn screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 1920,
            height: 1080,
        }
    }
    fn output_at(&self, _x: i32, _y: i32) -> Option<crate::backend::common_define::OutputId> {
        Some(crate::backend::common_define::OutputId(0))
    }
}

pub struct DummyKeyOps;
impl KeyOps for DummyKeyOps {
    fn grab_keys(&self, _root: WindowId, _bindings: &[(Mods, KeySym)]) -> Result<(), BackendError> {
        Ok(())
    }
    fn clear_key_grabs(&self, _root: WindowId) -> Result<(), BackendError> {
        Ok(())
    }
    fn clean_mods(&self, _raw: u16) -> Mods {
        Mods::empty()
    }
    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, BackendError> {
        Ok(keycode as u32)
    }
    fn clear_cache(&mut self) {}
}

pub struct DummyCursorProvider;
impl CursorProvider for DummyCursorProvider {
    fn preload_common(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
    fn get(&mut self, _kind: StdCursorKind) -> Result<CursorHandle, BackendError> {
        Ok(CursorHandle(0))
    }
    fn apply(&mut self, _win: WindowId, _kind: StdCursorKind) -> Result<(), BackendError> {
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
}

pub struct DummyColorAllocator;
impl ColorAllocator for DummyColorAllocator {
    fn set_scheme(&mut self, _t: SchemeType, _s: ColorScheme) {}
    fn allocate_schemes_pixels(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
    fn get_border_pixel_of(&mut self, _t: SchemeType) -> Result<Pixel, BackendError> {
        Ok(Pixel(0))
    }
    fn free_all_theme_pixels(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
}
