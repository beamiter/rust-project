// src/backend/x11/adapter.rs
use x11rb::protocol::xproto::{ButtonIndex, KeyButMask};
use crate::backend::common_input::{Mods, MouseButton};

pub fn mods_from_x11(mask: KeyButMask, numlock_mask: KeyButMask) -> Mods {
    let mut m = Mods::empty();
    let raw = mask.bits();

    if raw & KeyButMask::SHIFT.bits()   != 0 { m |= Mods::SHIFT;   }
    if raw & KeyButMask::CONTROL.bits() != 0 { m |= Mods::CONTROL; }
    if raw & KeyButMask::MOD1.bits()    != 0 { m |= Mods::ALT;     }
    if raw & KeyButMask::MOD2.bits()    != 0 { m |= Mods::MOD2;    }
    if raw & KeyButMask::MOD3.bits()    != 0 { m |= Mods::MOD3;    }
    if raw & KeyButMask::MOD4.bits()    != 0 { m |= Mods::SUPER;   }
    if raw & KeyButMask::MOD5.bits()    != 0 { m |= Mods::MOD5;    }
    if raw & KeyButMask::LOCK.bits()    != 0 { m |= Mods::CAPS;    }
    if raw & numlock_mask.bits()        != 0 { m |= Mods::NUMLOCK; }
    m
}

pub fn mods_to_x11(mods: Mods, numlock_mask: KeyButMask) -> KeyButMask {
    let mut m = KeyButMask::default();
    if mods.contains(Mods::SHIFT)   { m |= KeyButMask::SHIFT;   }
    if mods.contains(Mods::CONTROL) { m |= KeyButMask::CONTROL; }
    if mods.contains(Mods::ALT)     { m |= KeyButMask::MOD1;    }
    if mods.contains(Mods::MOD2)    { m |= KeyButMask::MOD2;    }
    if mods.contains(Mods::MOD3)    { m |= KeyButMask::MOD3;    }
    if mods.contains(Mods::SUPER)   { m |= KeyButMask::MOD4;    }
    if mods.contains(Mods::MOD5)    { m |= KeyButMask::MOD5;    }
    if mods.contains(Mods::CAPS)    { m |= KeyButMask::LOCK;    }
    if mods.contains(Mods::NUMLOCK) { m |= numlock_mask;        }
    m
}

pub fn button_from_x11(detail: u8) -> MouseButton {
    MouseButton::from_u8(detail)
}
pub fn button_to_x11(btn: MouseButton) -> ButtonIndex {
    ButtonIndex::from(btn.to_u8())
}
