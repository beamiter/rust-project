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
    pub const CENTERED_MASTER: Self = Self("centeredmaster");
    pub const BSTACK: Self = Self("bstack");
    pub const GRID: Self = Self("grid");
    pub const DECK: Self = Self("deck");
    pub const THREE_COL: Self = Self("threecol");
    pub const TATAMI: Self = Self("tatami");
    pub const FULLSCREEN: Self = Self("fullscreen");
    pub const ANY: Self = Self("");

    pub fn symbol(&self) -> &str {
        match self.0 {
            "tile" => "[]=",
            "float" => "><>",
            "monocle" => "[M]",
            "fibonacci" => "[@]",
            "centeredmaster" => "|M|",
            "bstack" => "TTT",
            "grid" => "HHH",
            "deck" => "[D]",
            "threecol" => "|||",
            "tatami" => "[+]",
            "fullscreen" => "[ ]",
            _ => "",
        }
    }

    pub fn is_tile(&self) -> bool {
        matches!(
            self.0,
            "tile" | "fibonacci" | "centeredmaster" | "bstack" | "grid" | "deck" | "threecol" | "tatami" | "fullscreen"
        )
    }
    pub fn is_float(&self) -> bool {
        self.0 == "float"
    }
    pub fn is_monocle(&self) -> bool {
        self.0 == "monocle" || self.0 == "fullscreen"
    }

    pub fn is_fullscreen_layout(&self) -> bool {
        self.0 == "fullscreen"
    }

    /// 所有布局的循环顺序
    const CYCLE: &'static [LayoutEnum] = &[
        Self::TILE,
        Self::FIBONACCI,
        Self::CENTERED_MASTER,
        Self::BSTACK,
        Self::GRID,
        Self::DECK,
        Self::THREE_COL,
        Self::TATAMI,
        Self::MONOCLE,
        Self::FULLSCREEN,
        Self::FLOAT,
    ];

    pub fn cycle_next(&self) -> &'static LayoutEnum {
        let idx = Self::CYCLE.iter().position(|l| l == self).unwrap_or(0);
        &Self::CYCLE[(idx + 1) % Self::CYCLE.len()]
    }

    pub fn cycle_prev(&self) -> &'static LayoutEnum {
        let idx = Self::CYCLE.iter().position(|l| l == self).unwrap_or(0);
        &Self::CYCLE[(idx + Self::CYCLE.len() - 1) % Self::CYCLE.len()]
    }
}

impl From<u32> for LayoutEnum {
    fn from(value: u32) -> Self {
        match value {
            0 => LayoutEnum::TILE,
            1 => LayoutEnum::FLOAT,
            2 => LayoutEnum::MONOCLE,
            3 => LayoutEnum::FIBONACCI,
            4 => LayoutEnum::CENTERED_MASTER,
            5 => LayoutEnum::BSTACK,
            6 => LayoutEnum::GRID,
            7 => LayoutEnum::DECK,
            8 => LayoutEnum::THREE_COL,
            9 => LayoutEnum::TATAMI,
            10 => LayoutEnum::FULLSCREEN,
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

/// Centered Master: Master 居中，Stack 分列两侧
pub fn calculate_centered_master<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len() as u32;
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(clients.len());
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    let n_master = params.n_master;
    let n_master_count = n.min(n_master) as i32;
    let n_stack = (n as i32 - n_master as i32).max(0);

    if n_stack == 0 {
        let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
        let mut my = 0;
        for (i, c) in clients.iter().enumerate() {
            let remaining = n_master_count - i as i32;
            let h = (master_avail_h - my).max(0) / remaining.max(1);
            let b2 = 2 * c.border_w;
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx, wy + my + i as i32 * gap, ww - b2, h - b2),
            });
            my += h;
        }
        return results;
    }

    // 左 stack | 中 master | 右 stack
    let mw = ((ww - 2 * gap) as f32 * params.m_fact) as i32;
    let n_left = (n_stack + 1) / 2;
    let n_right = n_stack - n_left;
    let left_w = (ww - mw - 2 * gap) / 2;
    let right_w = ww - mw - 2 * gap - left_w;

    let master_x = wx + left_w + gap;
    let right_x = master_x + mw + gap;

    let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
    let left_avail_h = wh - (n_left - 1).max(0) * gap;
    let right_avail_h = if n_right > 0 { wh - (n_right - 1).max(0) * gap } else { wh };

    let mut mi = 0i32;
    let mut li = 0i32;
    let mut ri = 0i32;
    let mut my = 0;
    let mut ly = 0;
    let mut ry = 0;

    for (i, c) in clients.iter().enumerate() {
        let b2 = 2 * c.border_w;
        if (i as u32) < n_master {
            let remaining = n_master_count - mi;
            let h = (master_avail_h - my).max(0) / remaining.max(1);
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(master_x, wy + my + mi * gap, mw - b2, h - b2),
            });
            my += h;
            mi += 1;
        } else {
            let stack_idx = (i as u32 - n_master) as i32;
            if stack_idx % 2 == 0 {
                let remaining = n_left - li;
                let h = (left_avail_h - ly).max(0) / remaining.max(1);
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(wx, wy + ly + li * gap, left_w - b2, h - b2),
                });
                ly += h;
                li += 1;
            } else {
                let remaining = n_right - ri;
                let h = (right_avail_h - ry).max(0) / remaining.max(1);
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(right_x, wy + ry + ri * gap, right_w - b2, h - b2),
                });
                ry += h;
                ri += 1;
            }
        }
    }

    results
}

/// Bottom Stack: Master 在上，Stack 横排在下
pub fn calculate_bstack<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len() as u32;
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(clients.len());
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    let n_master = params.n_master;
    let n_master_count = n.min(n_master) as i32;
    let n_stack = (n as i32 - n_master as i32).max(0);

    let has_stack = n_stack > 0 && n_master_count > 0;
    let mh = if has_stack {
        ((wh - gap) as f32 * params.m_fact) as i32
    } else {
        wh
    };

    let master_avail_w = ww - (n_master_count - 1).max(0) * gap;
    let stack_avail_w = if n_stack > 0 { ww - (n_stack - 1).max(0) * gap } else { 0 };

    let mut mx = 0;
    let mut sx = 0;
    let mut mi = 0i32;
    let mut si = 0i32;

    for (i, c) in clients.iter().enumerate() {
        let b2 = 2 * c.border_w;
        if (i as u32) < n_master {
            let remaining = n_master_count - mi;
            let w = (master_avail_w - mx).max(0) / remaining.max(1);
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx + mx + mi * gap, wy, w - b2, mh - b2),
            });
            mx += w;
            mi += 1;
        } else {
            let remaining = n_stack - si;
            let w = (stack_avail_w - sx).max(0) / remaining.max(1);
            let sh = wh - mh - gap;
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx + sx + si * gap, wy + mh + gap, w - b2, sh - b2),
            });
            sx += w;
            si += 1;
        }
    }

    results
}

/// Grid: 等大小网格排列
pub fn calculate_grid<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len();
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(n);
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    let cols = (n as f32).sqrt().ceil() as i32;
    let rows = (n as i32 + cols - 1) / cols;

    let cell_w = (ww - (cols - 1) * gap) / cols;
    let cell_h = (wh - (rows - 1) * gap) / rows;

    for (i, c) in clients.iter().enumerate() {
        let row = i as i32 / cols;
        let col = i as i32 % cols;
        let b2 = 2 * c.border_w;

        // 最后一行可能不满，拉宽填满
        let (cx, cw) = if row == rows - 1 {
            let last_row_count = n as i32 - row * cols;
            let last_cell_w = (ww - (last_row_count - 1) * gap) / last_row_count;
            let last_col = i as i32 - row * cols;
            (wx + last_col * (last_cell_w + gap), last_cell_w)
        } else {
            (wx + col * (cell_w + gap), cell_w)
        };

        results.push(LayoutResult {
            key: c.key,
            rect: Rect::new(cx, wy + row * (cell_h + gap), cw - b2, cell_h - b2),
        });
    }

    results
}

/// Deck: Master 在左，Stack 区所有窗口重叠
pub fn calculate_deck<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len() as u32;
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(clients.len());
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    let n_master = params.n_master;
    let n_master_count = n.min(n_master) as i32;

    let has_stack = n > n_master && n_master > 0;
    let mw = if has_stack {
        ((ww - gap) as f32 * params.m_fact) as i32
    } else {
        ww
    };

    let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
    let mut my = 0;

    for (i, c) in clients.iter().enumerate() {
        let b2 = 2 * c.border_w;
        if (i as u32) < n_master {
            let mi = i as i32;
            let remaining = n_master_count - mi;
            let h = (master_avail_h - my).max(0) / remaining.max(1);
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx, wy + my + mi * gap, mw - b2, h - b2),
            });
            my += h;
        } else {
            // Stack 区所有窗口重叠在同一位置
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx + mw + gap, wy, ww - mw - gap - b2, wh - b2),
            });
        }
    }

    results
}

/// Three Column: 左Stack | 中Master | 右Stack
pub fn calculate_three_col<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len() as u32;
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(clients.len());
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    let n_master = params.n_master;
    let n_master_count = n.min(n_master) as i32;
    let n_stack = (n as i32 - n_master as i32).max(0);

    if n_stack == 0 {
        let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
        let mut my = 0;
        for (i, c) in clients.iter().enumerate() {
            let remaining = n_master_count - i as i32;
            let h = (master_avail_h - my).max(0) / remaining.max(1);
            let b2 = 2 * c.border_w;
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx, wy + my + i as i32 * gap, ww - b2, h - b2),
            });
            my += h;
        }
        return results;
    }

    let mw = ((ww - 2 * gap) as f32 * params.m_fact) as i32;
    let side_w = (ww - mw - 2 * gap) / 2;
    let right_side_w = ww - mw - 2 * gap - side_w;

    let n_left = (n_stack + 1) / 2;
    let n_right = n_stack - n_left;

    let master_x = wx + side_w + gap;
    let right_x = master_x + mw + gap;

    let master_avail_h = wh - (n_master_count - 1).max(0) * gap;
    let left_avail_h = wh - (n_left - 1).max(0) * gap;
    let right_avail_h = if n_right > 0 { wh - (n_right - 1).max(0) * gap } else { wh };

    let mut mi = 0i32;
    let mut li = 0i32;
    let mut ri = 0i32;
    let mut my = 0;
    let mut ly = 0;
    let mut ry = 0;

    for (i, c) in clients.iter().enumerate() {
        let b2 = 2 * c.border_w;
        if (i as u32) < n_master {
            let remaining = n_master_count - mi;
            let h = (master_avail_h - my).max(0) / remaining.max(1);
            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(master_x, wy + my + mi * gap, mw - b2, h - b2),
            });
            my += h;
            mi += 1;
        } else {
            let stack_idx = (i as u32 - n_master) as i32;
            if stack_idx % 2 == 0 {
                let remaining = n_left - li;
                let h = (left_avail_h - ly).max(0) / remaining.max(1);
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(wx, wy + ly + li * gap, side_w - b2, h - b2),
                });
                ly += h;
                li += 1;
            } else {
                let remaining = n_right - ri;
                let h = (right_avail_h - ry).max(0) / remaining.max(1);
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(right_x, wy + ry + ri * gap, right_side_w - b2, h - b2),
                });
                ry += h;
                ri += 1;
            }
        }
    }

    results
}

/// Tatami: 日式榻榻米布局，根据窗口数量选择不同的排列图案
pub fn calculate_tatami<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let n = clients.len();
    if n == 0 {
        return Vec::new();
    }

    let mut results = Vec::with_capacity(n);
    let gap = params.gap;

    let wx = params.screen_area.x + gap;
    let wy = params.screen_area.y + gap;
    let ww = params.screen_area.w - 2 * gap;
    let wh = params.screen_area.h - 2 * gap;

    if n <= 4 {
        // 少量窗口直接铺
        match n {
            1 => {
                let b2 = 2 * clients[0].border_w;
                results.push(LayoutResult {
                    key: clients[0].key,
                    rect: Rect::new(wx, wy, ww - b2, wh - b2),
                });
            }
            2 => {
                let w = (ww - gap) / 2;
                for (i, c) in clients.iter().enumerate() {
                    let b2 = 2 * c.border_w;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(wx + i as i32 * (w + gap), wy, w - b2, wh - b2),
                    });
                }
            }
            3 => {
                let lw = (ww - gap) / 2;
                let rw = ww - lw - gap;
                let rh = (wh - gap) / 2;
                let b0 = 2 * clients[0].border_w;
                let b1 = 2 * clients[1].border_w;
                let b2 = 2 * clients[2].border_w;
                results.push(LayoutResult { key: clients[0].key, rect: Rect::new(wx, wy, lw - b0, wh - b0) });
                results.push(LayoutResult { key: clients[1].key, rect: Rect::new(wx + lw + gap, wy, rw - b1, rh - b1) });
                results.push(LayoutResult { key: clients[2].key, rect: Rect::new(wx + lw + gap, wy + rh + gap, rw - b2, wh - rh - gap - b2) });
            }
            4 => {
                let cw = (ww - gap) / 2;
                let ch = (wh - gap) / 2;
                for (i, c) in clients.iter().enumerate() {
                    let b2 = 2 * c.border_w;
                    let col = i as i32 % 2;
                    let row = i as i32 / 2;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(wx + col * (cw + gap), wy + row * (ch + gap), cw - b2, ch - b2),
                    });
                }
            }
            _ => {}
        }
    } else {
        // 5+ 窗口：分组，每组 5 个，交替使用两种榻榻米图案
        let mut idx = 0;
        let groups = (n + 4) / 5;
        let row_h = (wh - (groups as i32 - 1) * gap) / groups as i32;

        for g in 0..groups {
            let remaining = n - idx;
            let count = remaining.min(5);
            let gy = wy + g as i32 * (row_h + gap);

            if count < 5 {
                // 不足 5 个的尾部组用 grid 方式铺
                let cols = count as i32;
                let cw = (ww - (cols - 1) * gap) / cols;
                for j in 0..count {
                    let b2 = 2 * clients[idx + j].border_w;
                    let actual_w = if j as i32 == cols - 1 { ww - (cols - 1) * (cw + gap) } else { cw };
                    results.push(LayoutResult {
                        key: clients[idx + j].key,
                        rect: Rect::new(wx + j as i32 * (cw + gap), gy, actual_w - b2, row_h - b2),
                    });
                }
            } else {
                // 5 个窗口：经典榻榻米
                let top_h = (row_h - gap) / 2;
                let bot_h = row_h - top_h - gap;

                if g % 2 == 0 {
                    // 图案 A: 上 3 下 2
                    let tw = (ww - 2 * gap) / 3;
                    let tw_last = ww - 2 * (tw + gap);
                    let bw = (ww - gap) / 2;
                    let bw_last = ww - bw - gap;

                    let (b0, b1, b2, b3, b4) = (
                        2 * clients[idx].border_w, 2 * clients[idx + 1].border_w,
                        2 * clients[idx + 2].border_w, 2 * clients[idx + 3].border_w,
                        2 * clients[idx + 4].border_w,
                    );
                    results.push(LayoutResult { key: clients[idx].key, rect: Rect::new(wx, gy, tw - b0, top_h - b0) });
                    results.push(LayoutResult { key: clients[idx + 1].key, rect: Rect::new(wx + tw + gap, gy, tw - b1, top_h - b1) });
                    results.push(LayoutResult { key: clients[idx + 2].key, rect: Rect::new(wx + 2 * (tw + gap), gy, tw_last - b2, top_h - b2) });
                    results.push(LayoutResult { key: clients[idx + 3].key, rect: Rect::new(wx, gy + top_h + gap, bw - b3, bot_h - b3) });
                    results.push(LayoutResult { key: clients[idx + 4].key, rect: Rect::new(wx + bw + gap, gy + top_h + gap, bw_last - b4, bot_h - b4) });
                } else {
                    // 图案 B: 上 2 下 3
                    let tw = (ww - gap) / 2;
                    let tw_last = ww - tw - gap;
                    let bw = (ww - 2 * gap) / 3;
                    let bw_last = ww - 2 * (bw + gap);

                    let (b0, b1, b2, b3, b4) = (
                        2 * clients[idx].border_w, 2 * clients[idx + 1].border_w,
                        2 * clients[idx + 2].border_w, 2 * clients[idx + 3].border_w,
                        2 * clients[idx + 4].border_w,
                    );
                    results.push(LayoutResult { key: clients[idx].key, rect: Rect::new(wx, gy, tw - b0, top_h - b0) });
                    results.push(LayoutResult { key: clients[idx + 1].key, rect: Rect::new(wx + tw + gap, gy, tw_last - b1, top_h - b1) });
                    results.push(LayoutResult { key: clients[idx + 2].key, rect: Rect::new(wx, gy + top_h + gap, bw - b2, bot_h - b2) });
                    results.push(LayoutResult { key: clients[idx + 3].key, rect: Rect::new(wx + bw + gap, gy + top_h + gap, bw - b3, bot_h - b3) });
                    results.push(LayoutResult { key: clients[idx + 4].key, rect: Rect::new(wx + 2 * (bw + gap), gy + top_h + gap, bw_last - b4, bot_h - b4) });
                }
            }
            idx += count;
        }
    }

    results
}

/// Fullscreen: 真全屏，占满整个显示器，无边框无 gap
pub fn calculate_fullscreen<K: Copy>(
    params: &LayoutParams,
    clients: &[LayoutClient<K>],
) -> Vec<LayoutResult<K>> {
    let LayoutParams { screen_area, .. } = params;
    // screen_area 由调用方传入完整显示器区域 (m_x, m_y, m_w, m_h)
    clients
        .iter()
        .map(|c| LayoutResult {
            key: c.key,
            rect: Rect::new(screen_area.x, screen_area.y, screen_area.w, screen_area.h),
        })
        .collect()
}
