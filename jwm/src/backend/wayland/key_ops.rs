// src/backend/wayland/key_ops.rs
use crate::backend::api::KeyOps;
use crate::backend::common_define::{KeySym, Mods};
use std::collections::HashMap;

pub struct WlKeyOps {
    // 仅用于 JWM::keysym_from_keycode 的简单映射缓存（由输入事件动态更新）
    cache: HashMap<u8, KeySym>,
    // 简易NumLock掩码在后端无效，返回0
}

impl WlKeyOps {
    pub fn new() -> Self {
        Self { cache: HashMap::new() }
    }

    #[allow(dead_code)]
    pub fn update_cache(&mut self, keycode: u8, keysym: KeySym) {
        self.cache.insert(keycode, keysym);
    }
}

impl KeyOps for WlKeyOps {
    fn detect_numlock_mask(&mut self) -> Result<(Mods, u16), Box<dyn std::error::Error>> {
        Ok((Mods::NUMLOCK, 0)) // Wayland下 grab 用不到，JWM只读值
    }

    fn clear_key_grabs(&self, _root: crate::backend::common_define::WindowId) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland 无全局 grab，no-op
        Ok(())
    }

    fn grab_keys(
        &self,
        _root: crate::backend::common_define::WindowId,
        _bindings: &[(Mods, KeySym)],
        _numlock_mask_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Wayland 无全局 grab，no-op
        Ok(())
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>> {
        Ok(*self.cache.get(&keycode).unwrap_or(&0))
    }

    fn clear_cache(&mut self) { self.cache.clear(); }

    fn mods_from_raw_mask(&self, raw: u16, _numlock_mask_bits: u16) -> Mods {
        Mods::from_bits_truncate(raw) // 输入层会给出通用raw
    }

    fn backend_mods_mask_for_grab(&self, _mods: Mods, _numlock_mask_bits: u16) -> u16 {
        0 // Wayland不需要
    }
}
