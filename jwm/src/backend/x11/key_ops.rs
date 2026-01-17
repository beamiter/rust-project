// src/backend/x11/key_ops.rs
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;

use crate::backend::api::KeyOps;
use crate::backend::common_define::WindowId;
use crate::backend::common_define::{KeySym, Mods};
use crate::backend::error::BackendError;
use crate::backend::x11::adapter::mods_to_x11;
use crate::backend::x11::ids::X11IdRegistry;

pub struct X11KeyOps<C: Connection> {
    conn: Arc<C>,
    cache: HashMap<u8, u32>,
    numlock_mask: Arc<Mutex<u16>>,
    ids: X11IdRegistry,
}

impl<C: Connection> X11KeyOps<C> {
    pub fn new(conn: Arc<C>, numlock_mask: Arc<Mutex<u16>>, ids: X11IdRegistry) -> Self {
        let mut ops = Self {
            conn: conn.clone(),
            cache: HashMap::new(),
            numlock_mask,
            ids,
        };
        let _ = ops.detect_and_store_numlock();
        ops
    }

    fn detect_and_store_numlock(&mut self) -> Result<(), BackendError> {
        let numkc = self.find_numlock_keycode()?;
        let mask = if numkc == 0 {
            0
        } else {
            self.find_modifier_mask(numkc)? as u16
        };

        *self.numlock_mask.lock().unwrap() = mask;
        Ok(())
    }

    fn find_numlock_keycode(&self) -> Result<u8, BackendError> {
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

    fn find_modifier_mask(&self, target_keycode: u8) -> Result<u8, BackendError> {
        let mm = self.conn.get_modifier_mapping()?.reply()?;
        let per = mm.keycodes_per_modifier() as usize;
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
    fn clean_mods(&self, raw: u16) -> Mods {
        let numlock = *self.numlock_mask.lock().unwrap();
        let raw_mask = x11rb::protocol::xproto::KeyButMask::from(raw);
        let numlock_mask = x11rb::protocol::xproto::KeyButMask::from(numlock);
        crate::backend::x11::adapter::mods_from_x11(raw_mask, numlock_mask)
    }

    fn clear_key_grabs(&self, root: WindowId) -> Result<(), BackendError> {
        let r = self.ids.x11(root)?;
        self.conn.ungrab_key(Grab::ANY, r, ModMask::ANY.into())?;
        Ok(())
    }

    fn grab_keys(&self, root: WindowId, bindings: &[(Mods, KeySym)]) -> Result<(), BackendError> {
        let numlock_local = *self.numlock_mask.lock().unwrap();
        let r = self.ids.x11(root)?;

        let setup = self.conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let mapping = self
            .conn
            .get_keyboard_mapping(min, (max - min) + 1)?
            .reply()?;
        let per = mapping.keysyms_per_keycode as usize;

        use x11rb::protocol::xproto::{KeyButMask as KBM, ModMask};
        let numlock_mask_obj = KBM::from(numlock_local);

        for (mods, keysym) in bindings {
            for (offset, keysyms_for_keycode) in mapping.keysyms.chunks(per).enumerate() {
                let keycode = min + offset as u8;
                if let Some(&ks) = keysyms_for_keycode.first() {
                    if u32::from(ks) == *keysym {
                        let base = mods_to_x11(*mods, numlock_mask_obj);
                        let combos = [
                            base,
                            base | KBM::LOCK,
                            base | numlock_mask_obj,
                            base | KBM::LOCK | numlock_mask_obj,
                        ];
                        for mm in combos {
                            self.conn.grab_key(
                                false,
                                r,
                                ModMask::from(mm.bits()),
                                keycode,
                                GrabMode::ASYNC,
                                GrabMode::ASYNC,
                            )?;
                        }
                    }
                }
            }
        }

        self.conn.flush()?;
        Ok(())
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, BackendError> {
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
