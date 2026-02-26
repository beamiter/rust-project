// src/core/models.rs

use crate::backend::common_define::WindowId;
use crate::backend::api::LayerSurfaceInfo;
use crate::core::layout::LayoutEnum;
use slotmap::DefaultKey;
use std::fmt;
use std::rc::Rc;

pub type ClientKey = DefaultKey;
pub type MonitorKey = DefaultKey;

#[derive(Debug, Clone, PartialEq)]
pub struct WMClient {
    pub name: String,
    pub class: String,
    pub instance: String,
    pub win: WindowId,

    pub geometry: ClientGeometry,
    pub size_hints: SizeHints,

    pub state: ClientState,

    pub mon: Option<MonitorKey>,

    pub monitor_num: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientGeometry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub old_x: i32,
    pub old_y: i32,
    pub old_w: i32,
    pub old_h: i32,
    pub border_w: i32,
    pub old_border_w: i32,

    pub floating_x: i32,
    pub floating_y: i32,
    pub floating_w: i32,
    pub floating_h: i32,
}

impl fmt::Display for ClientGeometry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SizeHints {
    pub base_w: i32,
    pub base_h: i32,
    pub inc_w: i32,
    pub inc_h: i32,
    pub max_w: i32,
    pub max_h: i32,
    pub min_w: i32,
    pub min_h: i32,
    pub min_aspect: f32,
    pub max_aspect: f32,
    pub hints_valid: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientState {
    pub tags: u32,
    pub client_fact: f32,
    pub is_fixed: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
    pub never_focus: bool,
    pub old_state: bool,
    pub is_fullscreen: bool,
    pub is_sticky: bool,
    pub is_pip: bool,
    pub is_dock: bool,

    pub dock_layer_info: Option<LayerSurfaceInfo>,
}

impl WMClient {
    pub fn new(win: WindowId) -> Self {
        Self {
            name: String::new(),
            class: String::new(),
            instance: String::new(),
            win,
            geometry: ClientGeometry::default(),
            size_hints: SizeHints::default(),
            state: ClientState::default(),
            mon: None,
            monitor_num: 1000,
        }
    }

    pub fn total_width(&self) -> i32 {
        self.geometry.w + 2 * self.geometry.border_w
    }

    pub fn total_height(&self) -> i32 {
        self.geometry.h + 2 * self.geometry.border_w
    }

    pub fn is_status_bar(&self, status_bar_name: &str) -> bool {
        self.name == status_bar_name
            || self.class == status_bar_name
            || self.instance == status_bar_name
    }

    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (
            self.geometry.x,
            self.geometry.y,
            self.geometry.w,
            self.geometry.h,
        )
    }
}

impl fmt::Display for WMClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WMClient {{ name: \"{}\", class: \"{}\", instance: \"{}\", win: {:?}, geometry: {}, monitor: {} }}",
            self.name,
            self.class,
            self.instance,
            self.win,
            self.geometry,
            if self.mon.is_some() { "Some" } else { "None" }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WMMonitor {
    pub num: i32,
    pub lt_symbol: String,
    pub layout: MonitorLayout,
    pub geometry: MonitorGeometry,
    pub sel_tags: usize,
    pub sel_lt: usize,
    pub tag_set: [u32; 2],
    pub sel: Option<ClientKey>,
    pub lt: [Rc<LayoutEnum>; 2],
    pub pertag: Option<Pertag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorLayout {
    pub m_fact: f32,
    pub n_master: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorGeometry {
    pub m_x: i32,
    pub m_y: i32,
    pub m_w: i32,
    pub m_h: i32,
    pub w_x: i32,
    pub w_y: i32,
    pub w_w: i32,
    pub w_h: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pertag {
    pub cur_tag: usize,
    pub prev_tag: usize,
    pub n_masters: Vec<u32>,
    pub m_facts: Vec<f32>,
    pub sel_lts: Vec<usize>,
    pub lt_idxs: Vec<Vec<Option<Rc<LayoutEnum>>>>,
    pub show_bars: Vec<bool>,
    pub sel: Vec<Option<ClientKey>>,
}

impl Pertag {
    pub fn new(show_bar: bool, tags_length: usize) -> Self {
        let len = tags_length + 1;
        Self {
            cur_tag: 0,
            prev_tag: 0,
            n_masters: vec![0; len],
            m_facts: vec![0.; len],
            sel_lts: vec![0; len],
            lt_idxs: vec![vec![None; 2]; len],
            show_bars: vec![show_bar; len],
            sel: vec![None; len],
        }
    }
}

impl Default for MonitorLayout {
    fn default() -> Self {
        Self {
            m_fact: 0.55, // 默认主区域比例
            n_master: 1,  // 默认主窗口数量
        }
    }
}

impl WMMonitor {
    pub fn new() -> Self {
        Self {
            num: 0,
            lt_symbol: String::new(),
            layout: MonitorLayout {
                m_fact: 0.55,
                n_master: 1,
            },
            geometry: MonitorGeometry::default(),
            sel_tags: 0,
            sel_lt: 0,
            tag_set: [0; 2],
            sel: None,
            lt: [Rc::new(LayoutEnum::FIBONACCI), Rc::new(LayoutEnum::TILE)],
            pertag: None,
        }
    }

    pub fn intersect_area(&self, x: i32, y: i32, w: i32, h: i32) -> i32 {
        let geom = &self.geometry;
        std::cmp::max(
            0,
            std::cmp::min(x + w, geom.w_x + geom.w_w) - std::cmp::max(x, geom.w_x),
        ) * std::cmp::max(
            0,
            std::cmp::min(y + h, geom.w_y + geom.w_h) - std::cmp::max(y, geom.w_y),
        )
    }

    /// 切换到指定标签，返回新的 cur_tag 索引
    /// logic from: switch_to_tag & update_tagset_and_pertag
    pub fn view_tag(&mut self, target_tag_mask: u32, toggle: bool) -> usize {
        let tag_mask = if toggle {
            self.tag_set[self.sel_tags] ^ target_tag_mask
        } else {
            target_tag_mask
        };

        // 避免切换到空标签
        if tag_mask == 0 {
            return self.pertag.as_ref().map(|p| p.cur_tag).unwrap_or(1);
        }

        self.sel_tags ^= 1; // 切换当前激活的 tagset 索引 (0 或 1)
        self.tag_set[self.sel_tags] = tag_mask;

        // 计算新的 cur_tag 索引 (用于 Pertag)
        let new_cur_tag = if tag_mask == !0 {
            // 查看所有标签
            0
        } else {
            // 如果是单个标签，直接取索引 + 1
            // 如果是多个标签，且包含当前 Pertag 的标签，保持不变
            // 否则取第一个激活的标签
            let current_cur_tag = self.pertag.as_ref().map(|p| p.cur_tag).unwrap_or(1);

            if current_cur_tag > 0 && (tag_mask & (1 << (current_cur_tag - 1))) > 0 {
                current_cur_tag
            } else {
                // trailing_zeros 得到 0..31，加1对应 1..32
                (tag_mask.trailing_zeros() as usize) + 1
            }
        };

        self.apply_pertag_context(new_cur_tag);
        new_cur_tag
    }

    /// 应用 Pertag 上下文 (logic from: apply_pertag_settings/apply_pertag_settings_for_monitor)
    fn apply_pertag_context(&mut self, new_tag_idx: usize) {
        if let Some(ref mut pertag) = self.pertag {
            pertag.prev_tag = pertag.cur_tag;
            pertag.cur_tag = new_tag_idx;

            // 从 Pertag 恢复布局状态到 Monitor
            self.layout.n_master = pertag.n_masters[new_tag_idx];
            self.layout.m_fact = pertag.m_facts[new_tag_idx];
            self.sel_lt = pertag.sel_lts[new_tag_idx];

            if let Some(l0) = &pertag.lt_idxs[new_tag_idx][0] {
                self.lt[0] = l0.clone();
            }
            if let Some(l1) = &pertag.lt_idxs[new_tag_idx][1] {
                self.lt[1] = l1.clone();
            }
        }
        // 更新符号
        self.lt_symbol = self.lt[self.sel_lt].symbol().to_string();
    }

    /// 更新当前 Tag 的布局参数 (当 incnmaster 或 setmfact 时调用)
    pub fn update_current_tag_layout_params(&mut self) {
        if let Some(ref mut pertag) = self.pertag {
            let cur = pertag.cur_tag;
            pertag.n_masters[cur] = self.layout.n_master;
            pertag.m_facts[cur] = self.layout.m_fact;
        }
    }

    /// 更新当前 Tag 的 Layout 选择
    pub fn update_current_tag_layout_selection(&mut self) {
        if let Some(ref mut pertag) = self.pertag {
            let cur = pertag.cur_tag;
            let sel = self.sel_lt;
            pertag.sel_lts[cur] = sel;
            // 更新 layout 引用
            pertag.lt_idxs[cur][sel] = Some(self.lt[sel].clone());
        }
    }

    /// 获取当前 Tag 记录的选中客户端
    pub fn get_selected_client_for_current_tag(&self) -> Option<ClientKey> {
        self.pertag.as_ref().and_then(|p| p.sel[p.cur_tag])
    }

    /// 设置当前 Tag 的选中客户端
    pub fn set_selected_client_for_current_tag(&mut self, client: Option<ClientKey>) {
        if let Some(ref mut pertag) = self.pertag {
            pertag.sel[pertag.cur_tag] = client;
        }
        self.sel = client;
    }
}

impl fmt::Display for WMMonitor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WMMonitor {{ num: {}, geometry: {:?}, sel: {} }}",
            self.num,
            self.geometry,
            self.sel.is_some()
        )
    }
}
