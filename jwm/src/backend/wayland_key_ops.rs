use std::collections::HashMap;

use crate::backend::api::KeyOps;
use crate::backend::common_define::{KeySym, Mods, WindowId};
use crate::backend::error::BackendError;

use xkbcommon::xkb;

/// Keyboard helpers for the udev/libinput backend.
///
/// JWM's keybinding matching expects an *unmodified* keysym (like X11's "level 0" mapping),
/// while modifiers are matched separately via `Mods`.
pub struct UdevKeyOps {
    #[allow(dead_code)]
    context: xkb::Context,
    #[allow(dead_code)]
    keymap: xkb::Keymap,
    base_state: xkb::State,
    cache: HashMap<u8, KeySym>,
}

// NOTE: xkbcommon types are not marked Send due to raw pointers internally.
// JWM's udev backend is single-threaded and `UdevKeyOps` never crosses threads,
// so this is safe under that assumption.
unsafe impl Send for UdevKeyOps {}

impl UdevKeyOps {
    pub fn new() -> Result<Self, BackendError> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

        // Build a keymap from the conventional environment variables used by wlroots/sway/etc.
        // This makes keyboard mapping work reliably on TTY + udev.
        //
        // If the user doesn't provide anything, fall back to common defaults.
        fn env_nonempty(key: &str) -> Option<String> {
            std::env::var(key).ok().and_then(|v| {
                let v = v.trim().to_string();
                if v.is_empty() { None } else { Some(v) }
            })
        }

        let rules = env_nonempty("XKB_DEFAULT_RULES").unwrap_or_else(|| "evdev".to_string());
        let model = env_nonempty("XKB_DEFAULT_MODEL").unwrap_or_else(|| "pc105".to_string());
        let layout = env_nonempty("XKB_DEFAULT_LAYOUT").unwrap_or_else(|| "us".to_string());
        let variant = env_nonempty("XKB_DEFAULT_VARIANT").unwrap_or_default();
        let options = env_nonempty("XKB_DEFAULT_OPTIONS");

        log::info!(
            "xkb keymap: rules={rules:?} model={model:?} layout={layout:?} variant={variant:?} options={options:?}"
        );

        let keymap = xkb::Keymap::new_from_names(
            &context,
            &rules,
            &model,
            &layout,
            &variant,
            options,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| BackendError::Message("xkb keymap creation failed".into()))?;

        let base_state = xkb::State::new(&keymap);

        Ok(Self {
            context,
            keymap,
            base_state,
            cache: HashMap::new(),
        })
    }

    fn keysym_from_xkb_keycode_uncached(&self, keycode: u8) -> KeySym {
        let kc = xkb::Keycode::new(keycode as u32);
        let sym = self.base_state.key_get_one_sym(kc);
        sym.raw()
    }
}

impl KeyOps for UdevKeyOps {
    fn grab_keys(&self, _root: WindowId, _bindings: &[(Mods, KeySym)]) -> Result<(), BackendError> {
        // No global key grabbing in the udev backend (handled by the compositor input path).
        Ok(())
    }

    fn clear_key_grabs(&self, _root: WindowId) -> Result<(), BackendError> {
        Ok(())
    }

    fn clean_mods(&self, raw_state: u16) -> Mods {
        // For the udev backend, `raw_state` is already stored as JWM's `Mods` bitflags.
        Mods::from_bits_truncate(raw_state)
    }

    fn keysym_from_keycode(&mut self, keycode: u8) -> Result<KeySym, BackendError> {
        if let Some(&ks) = self.cache.get(&keycode) {
            return Ok(ks);
        }

        let sym = self.keysym_from_xkb_keycode_uncached(keycode);
        self.cache.insert(keycode, sym);
        Ok(sym)
    }

    fn clear_cache(&mut self) {
        self.cache.clear();
    }
}
