mod pangoxft_sys {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use pangoxft_sys::pango_xft_render_layout;
pub use pangoxft_sys::pango_xft_get_font_map;

