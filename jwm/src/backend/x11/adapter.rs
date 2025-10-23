// src/backend/x11/adapter.rs
use crate::backend::common_define::MouseButton;
use crate::backend::common_define::{EventMaskBits, Mods};
use x11rb::protocol::xproto::{ButtonIndex, EventMask, KeyButMask};

pub fn mods_from_x11(mask: KeyButMask, numlock_mask: KeyButMask) -> Mods {
    let mut m = Mods::empty();
    let raw = mask.bits();

    if raw & KeyButMask::SHIFT.bits() != 0 {
        m |= Mods::SHIFT;
    }
    if raw & KeyButMask::CONTROL.bits() != 0 {
        m |= Mods::CONTROL;
    }
    if raw & KeyButMask::MOD1.bits() != 0 {
        m |= Mods::ALT;
    }
    if raw & KeyButMask::MOD2.bits() != 0 {
        m |= Mods::MOD2;
    }
    if raw & KeyButMask::MOD3.bits() != 0 {
        m |= Mods::MOD3;
    }
    if raw & KeyButMask::MOD4.bits() != 0 {
        m |= Mods::SUPER;
    }
    if raw & KeyButMask::MOD5.bits() != 0 {
        m |= Mods::MOD5;
    }
    if raw & KeyButMask::LOCK.bits() != 0 {
        m |= Mods::CAPS;
    }
    if raw & numlock_mask.bits() != 0 {
        m |= Mods::NUMLOCK;
    }
    m
}

pub fn mods_to_x11(mods: Mods, numlock_mask: KeyButMask) -> KeyButMask {
    let mut m = KeyButMask::default();
    if mods.contains(Mods::SHIFT) {
        m |= KeyButMask::SHIFT;
    }
    if mods.contains(Mods::CONTROL) {
        m |= KeyButMask::CONTROL;
    }
    if mods.contains(Mods::ALT) {
        m |= KeyButMask::MOD1;
    }
    if mods.contains(Mods::MOD2) {
        m |= KeyButMask::MOD2;
    }
    if mods.contains(Mods::MOD3) {
        m |= KeyButMask::MOD3;
    }
    if mods.contains(Mods::SUPER) {
        m |= KeyButMask::MOD4;
    }
    if mods.contains(Mods::MOD5) {
        m |= KeyButMask::MOD5;
    }
    if mods.contains(Mods::CAPS) {
        m |= KeyButMask::LOCK;
    }
    if mods.contains(Mods::NUMLOCK) {
        m |= numlock_mask;
    }
    m
}

pub fn button_from_x11(detail: u8) -> MouseButton {
    MouseButton::from_u8(detail)
}
pub fn button_to_x11(btn: MouseButton) -> ButtonIndex {
    ButtonIndex::from(btn.to_u8())
}

pub fn event_mask_from_generic(bits: u32) -> EventMask {
    let mut m = EventMask::default();
    if (bits & EventMaskBits::BUTTON_PRESS.bits()) != 0 {
        m |= EventMask::BUTTON_PRESS;
    }
    if (bits & EventMaskBits::BUTTON_RELEASE.bits()) != 0 {
        m |= EventMask::BUTTON_RELEASE;
    }
    if (bits & EventMaskBits::POINTER_MOTION.bits()) != 0 {
        m |= EventMask::POINTER_MOTION;
    }
    if (bits & EventMaskBits::ENTER_WINDOW.bits()) != 0 {
        m |= EventMask::ENTER_WINDOW;
    }
    if (bits & EventMaskBits::LEAVE_WINDOW.bits()) != 0 {
        m |= EventMask::LEAVE_WINDOW;
    }
    if (bits & EventMaskBits::PROPERTY_CHANGE.bits()) != 0 {
        m |= EventMask::PROPERTY_CHANGE;
    }
    if (bits & EventMaskBits::STRUCTURE_NOTIFY.bits()) != 0 {
        m |= EventMask::STRUCTURE_NOTIFY;
    }
    if (bits & EventMaskBits::SUBSTRUCTURE_REDIRECT.bits()) != 0 {
        m |= EventMask::SUBSTRUCTURE_REDIRECT;
    }
    if (bits & EventMaskBits::FOCUS_CHANGE.bits()) != 0 {
        m |= EventMask::FOCUS_CHANGE;
    }
    m
}
