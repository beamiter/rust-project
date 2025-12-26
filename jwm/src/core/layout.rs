// src/core/layout.rs
use super::types::Rect;

// 用于布局计算的客户端信息
pub struct LayoutClient<K> {
    pub key: K,        // ClientKey, 用于标识
    pub factor: f32,   // client_fact
    pub border_w: i32, // border width
}

pub struct LayoutParams {
    pub screen_area: Rect, // 可用区域 (已扣除 bar)
    pub n_master: u32,
    pub m_fact: f32,
}

// 通用布局结果
pub struct LayoutResult<K> {
    pub key: K,
    pub rect: Rect,
}

pub fn calculate_tile<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len() as u32;
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(clients.len());
    let LayoutParams {
        screen_area,
        n_master,
        m_fact,
    } = params;
    let (wx, wy, ww, wh) = (screen_area.x, screen_area.y, screen_area.w, screen_area.h);

    // 1. 计算总的 factors
    let (total_m_fact, total_s_fact) =
        clients
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(m, s), (i, c)| {
                if i < *n_master as usize {
                    (m + c.factor, s)
                } else {
                    (m, s + c.factor)
                }
            });

    // 2. 计算 Master 区域宽度
    let mw = if n > *n_master && *n_master > 0 {
        (ww as f32 * m_fact) as i32
    } else {
        ww
    };

    let mut my = 0; // Master Y offset
    let mut ty = 0; // Stack Y offset
    let mut remaining_m_fact = total_m_fact;
    let mut remaining_s_fact = total_s_fact;

    for (i, c) in clients.iter().enumerate() {
        let is_master = i < *n_master as usize;

        let (x, y, w, h) = if is_master {
            let remaining_masters = *n_master - i as u32;
            let remaining_h = (wh - my).max(0);

            let h = if remaining_m_fact > 0.001 {
                (remaining_h as f32 * (c.factor / remaining_m_fact)) as i32
            } else if remaining_masters > 0 {
                remaining_h / remaining_masters as i32
            } else {
                remaining_h
            };

            let res_y = wy + my;
            my += h;
            remaining_m_fact -= c.factor;

            (wx, res_y, mw, h)
        } else {
            let stack_idx = i - *n_master as usize;
            let stack_count = clients.len() - *n_master as usize;
            let remaining_stacks = stack_count - stack_idx;
            let remaining_h = (wh - ty).max(0);

            let h = if remaining_s_fact > 0.001 {
                (remaining_h as f32 * (c.factor / remaining_s_fact)) as i32
            } else if remaining_stacks > 0 {
                remaining_h / remaining_stacks as i32
            } else {
                remaining_h
            };

            let res_y = wy + ty;
            ty += h;
            remaining_s_fact -= c.factor;

            (wx + mw, res_y, ww - mw, h)
        };

        // 减去边框宽度
        results.push(LayoutResult {
            key: c.key,
            rect: Rect::new(x, y, w - 2 * c.border_w, h - 2 * c.border_w),
        });
    }

    results
}

pub fn calculate_monocle<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let LayoutParams { screen_area, .. } = params;
    let (wx, wy, ww, wh) = (screen_area.x, screen_area.y, screen_area.w, screen_area.h);

    clients
        .iter()
        .map(|c| LayoutResult {
            key: c.key,
            rect: Rect::new(wx, wy, ww - 2 * c.border_w, wh - 2 * c.border_w),
        })
        .collect()
}
