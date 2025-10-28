// src/backend/wayland/key_ops.rs
use crate::backend::api::KeyOps;
use crate::backend::common_define::{KeySym, Mods};

#[derive(Clone)]
pub struct KeyboardController;
impl KeyboardController {
    pub fn new() -> Self {
        Self
    }
}

pub struct WaylandKeyOps {
    _ctrl: KeyboardController,
    cache: std::collections::HashMap<u8, KeySym>,
}

impl WaylandKeyOps {
    pub fn new(ctrl: KeyboardController) -> Self {
        Self {
            _ctrl: ctrl,
            cache: Default::default(),
        }
    }
}

impl KeyOps for WaylandKeyOps {
    fn detect_numlock_mask(&mut self) -> Result<(Mods, u16), Box<dyn std::error::Error>> {
        // Wayland：通用语义，直接返回 NUMLOCK（raw 与 Mods 对齐）
        Ok((Mods::NUMLOCK, Mods::NUMLOCK.bits()))
    }

    fn clear_key_grabs(
        &self,
        _root: crate::backend::api::WindowId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland 无键抓取
        Ok(())
    }

    fn grab_keys(
        &self,
        _root: crate::backend::api::WindowId,
        _bindings: &[(Mods, KeySym)],
        _numlock_mask_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland 无键抓取
        Ok(())
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>> {
        // 简化：如果未缓存，直接返回 keycode 作为 keysym（可后续接入 xkbcommon）
        let v = *self.cache.entry(keycode).or_insert(keycode as u32);
        Ok(v)
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
    }

    fn mods_from_raw_mask(&self, raw: u16, _numlock_mask_bits: u16) -> Mods {
        Mods::from_bits_truncate(raw)
    }

    fn backend_mods_mask_for_grab(&self, _mods: Mods, _numlock_mask_bits: u16) -> u16 {
        0 // Wayland 无按钮抓取
    }
}
