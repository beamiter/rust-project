// src/core/layout.rs
use super::types::Rect;

// 用于布局计算的客户端信息
pub struct LayoutClient<K> {
    pub key: K,        // ClientKey, 用于标识
    pub factor: f32,   // client_fact
    pub border_w: i32, // border width
}

pub struct LayoutParams {
    pub screen_area: Rect,
    pub n_master: u32,
    pub m_fact: f32,
    pub gap: i32,
}

// 通用布局结果
pub struct LayoutResult<K> {
    pub key: K,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEnum(pub &'static str);

impl LayoutEnum {
    pub const TILE: Self = Self("tile");
    pub const FLOAT: Self = Self("float");
    pub const MONOCLE: Self = Self("monocle");
    pub const FIBONACCI: Self = Self("fibonacci");
    pub const ANY: Self = Self("");

    pub fn symbol(&self) -> &str {
        match self.0 {
            "tile" => "[]=",
            "float" => "><>",
            "monocle" => "[M]",
            "fibonacci" => "[@]",
            _ => "",
        }
    }

    pub fn is_tile(&self) -> bool {
        self.0 == "tile" || self.0 == "fibonacci"
    }
    pub fn is_float(&self) -> bool {
        self.0 == "float"
    }
    pub fn is_monocle(&self) -> bool {
        self.0 == "monocle"
    }
}

impl From<u32> for LayoutEnum {
    fn from(value: u32) -> Self {
        match value {
            0 => LayoutEnum::TILE,
            1 => LayoutEnum::FLOAT,
            2 => LayoutEnum::MONOCLE,
            3 => LayoutEnum::FIBONACCI,
            _ => LayoutEnum::ANY,
        }
    }
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
        gap,
    } = params;
    let gap = *gap;

    // 外边距：缩小可用区域
    let wx = screen_area.x + gap;
    let wy = screen_area.y + gap;
    let ww = screen_area.w - 2 * gap;
    let wh = screen_area.h - 2 * gap;

    let border2 = 2 * clients.first().map_or(0, |c| c.border_w);

    let has_stack = n > *n_master && *n_master > 0;
    // Master 和 Stack 列之间留 gap
    let mw = if has_stack {
        ((ww - gap) as f32 * m_fact) as i32
    } else {
        ww
    };

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

    let n_master_count = n.min(*n_master) as i32;
    let n_stack_count = (clients.len() as i32 - *n_master as i32).max(0);

    // Master 列中 N 个窗口之间有 (N-1) 个 gap
    let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
    // Stack 列同理
    let stack_avail_h = wh - (n_stack_count - 1).max(0) * gap;

    let mut mi = 0; // Master 窗口序号
    let mut si = 0; // Stack 窗口序号
    let mut my = 0; // Master Y offset
    let mut ty = 0; // Stack Y offset
    let mut remaining_m_fact = total_m_fact;
    let mut remaining_s_fact = total_s_fact;

    for (i, c) in clients.iter().enumerate() {
        let is_master = i < *n_master as usize;

        let (x, y, w, h) = if is_master {
            let remaining_masters = n_master_count - mi;
            let remaining_h = (master_avail_h - my).max(0);

            let h = if remaining_m_fact > 0.001 {
                (remaining_h as f32 * (c.factor / remaining_m_fact)) as i32
            } else if remaining_masters > 0 {
                remaining_h / remaining_masters
            } else {
                remaining_h
            };

            let res_y = wy + my + mi * gap;
            my += h;
            mi += 1;
            remaining_m_fact -= c.factor;

            (wx, res_y, mw - border2, h - border2)
        } else {
            let remaining_stacks = n_stack_count - si;
            let remaining_h = (stack_avail_h - ty).max(0);

            let h = if remaining_s_fact > 0.001 {
                (remaining_h as f32 * (c.factor / remaining_s_fact)) as i32
            } else if remaining_stacks > 0 {
                remaining_h / remaining_stacks
            } else {
                remaining_h
            };

            let res_y = wy + ty + si * gap;
            ty += h;
            si += 1;
            remaining_s_fact -= c.factor;

            (wx + mw + gap, res_y, ww - mw - gap - border2, h - border2)
        };

        results.push(LayoutResult {
            key: c.key,
            rect: Rect::new(x, y, w, h),
        });
    }

    results
}

pub fn calculate_monocle<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let LayoutParams { screen_area, .. } = params;
    // monocle 模式不使用 gap，窗口占满整个工作区
    let (wx, wy, ww, wh) = (screen_area.x, screen_area.y, screen_area.w, screen_area.h);

    clients
        .iter()
        .map(|c| {
            let border2 = 2 * c.border_w;
            LayoutResult {
                key: c.key,
                rect: Rect::new(wx, wy, ww - border2, wh - border2),
            }
        })
        .collect()
}

pub fn calculate_fibonacci<K: Copy>(
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
        gap,
    } = params;
    let gap = *gap;

    // 外边距
    let wx = screen_area.x + gap;
    let wy = screen_area.y + gap;
    let ww = screen_area.w - 2 * gap;
    let wh = screen_area.h - 2 * gap;

    let has_stack = n > *n_master;
    let mw = if has_stack {
        ((ww - gap) as f32 * m_fact) as i32
    } else {
        ww
    };

    let n_master_count = n.min(*n_master) as i32;
    let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
    let mut mi = 0i32;
    let mut my = 0;

    // Stack 区域的初始状态
    let mut sx = if *n_master > 0 { wx + mw + gap } else { wx };
    let mut sy = wy;
    let mut sw = if *n_master > 0 { ww - mw - gap } else { ww };
    let mut sh = wh;

    for (i, c) in clients.iter().enumerate() {
        let is_master = (i as u32) < *n_master;
        let border2 = 2 * c.border_w;

        if is_master {
            let remaining_masters = n_master_count - mi;
            let remaining_h = (master_avail_h - my).max(0);

            let h = if remaining_masters > 0 {
                remaining_h / remaining_masters
            } else {
                remaining_h
            };

            let res_y = wy + my + mi * gap;
            my += h;
            mi += 1;

            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx, res_y, mw - border2, h - border2),
            });
        } else {
            let stack_idx = (i as u32) - *n_master;
            let stack_count = n - *n_master;

            if stack_idx == stack_count - 1 {
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(sx, sy, sw - border2, sh - border2),
                });
            } else {
                if stack_idx % 2 == 0 {
                    // 水平分割
                    let h = (sh - gap) / 2;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(sx, sy, sw - border2, h - border2),
                    });
                    sy += h + gap;
                    sh -= h + gap;
                } else {
                    // 垂直分割
                    let w = (sw - gap) / 2;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(sx, sy, w - border2, sh - border2),
                    });
                    sx += w + gap;
                    sw -= w + gap;
                }
            }
        }
    }

    results
}
