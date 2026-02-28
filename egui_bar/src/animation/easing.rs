use crate::config::EasingName;

pub fn linear(t: f32) -> f32 {
    t
}

pub fn ease_in_quad(t: f32) -> f32 {
    t * t
}

pub fn ease_out_quad(t: f32) -> f32 {
    t * (2.0 - t)
}

pub fn ease_in_out_quad(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

pub fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t - 1.0;
    t * t * t + 1.0
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let t = t - 1.0;
        1.0 + 4.0 * t * t * t
    }
}

/// Get easing function by name
pub fn from_name(name: EasingName) -> fn(f32) -> f32 {
    match name {
        EasingName::Linear => linear,
        EasingName::EaseInQuad => ease_in_quad,
        EasingName::EaseOutQuad => ease_out_quad,
        EasingName::EaseInOutQuad => ease_in_out_quad,
        EasingName::EaseInCubic => ease_in_cubic,
        EasingName::EaseOutCubic => ease_out_cubic,
        EasingName::EaseInOutCubic => ease_in_out_cubic,
    }
}
