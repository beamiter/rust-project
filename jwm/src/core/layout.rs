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

        // 边框由 X server (X11) 或 compositor (Wayland) 管理，不从平铺尺寸扣除
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
    let (wx, wy, ww, wh) = (screen_area.x, screen_area.y, screen_area.w, screen_area.h);

    clients
        .iter()
        .map(|c| LayoutResult {
            key: c.key,
            rect: Rect::new(wx, wy, ww, wh),
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
    } = params;

    // 初始屏幕区域
    let (wx, wy, ww, wh) = (screen_area.x, screen_area.y, screen_area.w, screen_area.h);

    // 计算 Master 区域宽度
    // 如果有 Stack 窗口，Master 宽度受 m_fact 控制，否则占满全屏
    let mw = if n > *n_master {
        (ww as f32 * m_fact) as i32
    } else {
        ww
    };

    let mut my = 0; // Master Y 偏移

    // Stack 区域的初始状态
    // 如果有 Master，Stack 从 Master 右侧开始，宽度为剩余宽度
    // 如果没有 Master (n_master=0)，Stack 占满全屏
    let mut sx = if *n_master > 0 { wx + mw } else { wx };
    let mut sy = wy;
    let mut sw = if *n_master > 0 { ww - mw } else { ww };
    let mut sh = wh;

    for (i, c) in clients.iter().enumerate() {
        let is_master = (i as u32) < *n_master;

        if is_master {
            // --- Master 区域处理 (垂直列表) ---
            let remaining_masters = *n_master - i as u32;
            // 剩余高度
            let remaining_h = (wh - my).max(0);

            // 计算当前 Master 窗口高度 (平均分配)
            let h = if remaining_masters > 0 {
                remaining_h / remaining_masters as i32
            } else {
                remaining_h
            };

            let res_y = wy + my;
            my += h;

            results.push(LayoutResult {
                key: c.key,
                rect: Rect::new(wx, res_y, mw, h),
            });
        } else {
            // --- Stack 区域处理 (Fibonacci 螺旋) ---
            // 堆栈中的第几个元素 (从 0 开始)
            let stack_idx = (i as u32) - *n_master;
            let stack_count = n - *n_master;

            // 如果是堆栈中最后一个窗口，占据剩余所有空间
            if stack_idx == stack_count - 1 {
                results.push(LayoutResult {
                    key: c.key,
                    rect: Rect::new(sx, sy, sw, sh),
                });
            } else {
                // 确定切割方向：偶数水平切割，奇数垂直切割 (或者反过来，看个人喜好)
                // 这里采用：
                // stack_idx % 2 == 0 -> 上下分割 (当前窗口取上半部分)
                // stack_idx % 2 != 0 -> 左右分割 (当前窗口取左半部分)
                // 这样会形成：右 -> 下 -> 右 -> 下 的螺旋效果 (相对于上级容器)
                // 但因为我们是在剩余空间切，实际效果是 Dwindle

                if stack_idx % 2 == 0 {
                    // 水平分割：当前窗口取高度的一半
                    let h = sh / 2;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(sx, sy, sw, h),
                    });
                    // 更新剩余空间：Y 下移，高度减半
                    sy += h;
                    sh -= h;
                } else {
                    // 垂直分割：当前窗口取宽度的一半
                    let w = sw / 2;
                    results.push(LayoutResult {
                        key: c.key,
                        rect: Rect::new(sx, sy, w, sh),
                    });
                    // 更新剩余空间：X 右移，宽度减半
                    sx += w;
                    sw -= w;
                }
            }
        }
    }

    results
}
