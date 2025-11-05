use crate::backend::api::{KeyOps, KeySym, Mods, WindowId};

pub struct WaylandKeyOps {}

impl WaylandKeyOps {
    pub fn new() -> Self {
        Self {}
    }
}

#[allow(unused)]
impl KeyOps for WaylandKeyOps {
    fn detect_numlock_mask(&mut self) -> Result<(Mods, u16), Box<dyn std::error::Error>> {
        Ok((Mods::NONE, 0))
    }

    fn clear_key_grabs(&self, root: WindowId) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn grab_keys(
        &self,
        root: WindowId,
        bindings: &[(Mods, KeySym)],
        numlock_mask_bits: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, Box<dyn std::error::Error>> {
        Ok(0)
    }

    fn clear_cache(&mut self) {}

    fn mods_from_raw_mask(&self, raw: u16, numlock_mask_bits: u16) -> Mods {
        Mods::NONE
    }

    fn backend_mods_mask_for_grab(&self, mods: Mods, numlock_mask_bits: u16) -> u16 {
        0
    }
}
