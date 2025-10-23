// src/backend/x11/key_ops.rs
use std::collections::HashMap;
use std::sync::Arc;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::backend::api::{KeyOps, WindowId};
use crate::backend::common_define::{KeySym, Mods};
use crate::backend::x11::adapter::{mods_from_x11, mods_to_x11};

pub struct X11KeyOps<C: Connection> {
    conn: Arc<C>,
    // 简单缓存：keycode -> keysym
    cache: HashMap<u8, u32>,
}

impl<C: Connection> X11KeyOps<C> {
    pub fn new(conn: Arc<C>) -> Self {
        Self {
            conn,
            cache: HashMap::new(),
        }
    }

    fn find_numlock_keycode(&self) -> Result<u8, Box<dyn std::error::Error>> {
        const XK_NUM_LOCK: u32 = 0xFF7F;
        let setup = self.conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let mapping = self
            .conn
            .get_keyboard_mapping(min, (max - min) + 1)?
            .reply()?;
        let per = mapping.keysyms_per_keycode as usize;

        for kc in min..=max {
            let idx = (kc - min) as usize * per;
            if idx < mapping.keysyms.len() {
                for i in 0..per {
                    if mapping.keysyms[idx + i] == XK_NUM_LOCK {
                        return Ok(kc);
                    }
                }
            }
        }
        Ok(0)
    }

    fn find_modifier_mask(&self, target_keycode: u8) -> Result<u8, Box<dyn std::error::Error>> {
        let mm = self.conn.get_modifier_mapping()?.reply()?;
        let per = mm.keycodes_per_modifier() as usize;
        // 8 个修饰组：Shift, Lock, Control, Mod1..Mod5
        for mod_index in 0..8 {
            let start = mod_index * per;
            let end = start + per;
            if end <= mm.keycodes.len() {
                for &kc in &mm.keycodes[start..end] {
                    if kc == target_keycode && kc != 0 {
                        return Ok(1 << mod_index);
                    }
                }
            }
        }
        Ok(0)
    }
}

impl<C: Connection + Send + Sync + 'static> KeyOps for X11KeyOps<C> {
    fn mods_from_raw_mask(&self, raw: u16, numlock_mask_bits: u16) -> Mods {
        let raw_mask = KeyButMask::from(raw);
        let numlock = KeyButMask::from(numlock_mask_bits);
        mods_from_x11(raw_mask, numlock)
    }

    fn backend_mods_mask_for_grab(&self, mods: Mods, numlock_mask_bits: u16) -> u16 {
        let numlock = KeyButMask::from(numlock_mask_bits);
        mods_to_x11(mods, numlock).bits()
    }

    fn detect_numlock_mask(&mut self) -> Result<(Mods, u16), Box<dyn std::error::Error>> {
        let numkc = self.find_numlock_keycode()?;
        if numkc == 0 {
            // 回退：使用 MOD2 语义，并无 X11 掩码位
            Ok((Mods::MOD2, 0))
        } else {
            let m = self.find_modifier_mask(numkc)?;
            if m != 0 {
                Ok((Mods::NUMLOCK, m as u16))
            } else {
                Ok((Mods::empty(), 0))
            }
        }
    }

    fn clear_key_grabs(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        self.conn
            .ungrab_key(Grab::ANY, root.0 as u32, ModMask::ANY.into())?
            .check()?;
        Ok(())
    }

    fn grab_keys(
        &self,
        root: WindowId,
        bindings: &[(Mods, KeySym)],
        numlock_mask_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let setup = self.conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let mapping = self
            .conn
            .get_keyboard_mapping(min, (max - min) + 1)?
            .reply()?;
        let per = mapping.keysyms_per_keycode as usize;

        use x11rb::protocol::xproto::{KeyButMask as KBM, ModMask};
        let numlock_mask = KBM::from(numlock_mask_bits);

        for (mods, keysym) in bindings {
            // 遍历 keycodes 找到匹配的 keysym（first keysym）
            for (offset, keysyms_for_keycode) in mapping.keysyms.chunks(per).enumerate() {
                let keycode = min + offset as u8;
                if let Some(&ks) = keysyms_for_keycode.first() {
                    if u32::from(ks) == *keysym {
                        // 组合 None / LOCK / NUMLOCK / LOCK|NUMLOCK
                        let base = mods_to_x11(*mods, numlock_mask);
                        let combos = [
                            base,
                            base | KBM::LOCK,
                            base | numlock_mask,
                            base | KBM::LOCK | numlock_mask,
                        ];
                        for mm in combos {
                            self.conn
                                .grab_key(
                                    false,
                                    root.0 as u32,
                                    ModMask::from(mm.bits()),
                                    keycode,
                                    GrabMode::ASYNC,
                                    GrabMode::ASYNC,
                                )?
                                .check()?;
                        }
                    }
                }
            }
        }

        self.conn.flush()?;
        Ok(())
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>> {
        if let Some(&ks) = self.cache.get(&keycode) {
            return Ok(ks);
        }
        let mapping = self.conn.get_keyboard_mapping(keycode, 1)?.reply()?;
        let ks = mapping.keysyms.get(0).copied().unwrap_or(0);
        self.cache.insert(keycode, ks);
        Ok(ks)
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
    }
}
