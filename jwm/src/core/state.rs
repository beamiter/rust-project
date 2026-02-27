// src/core/state.rs
use crate::backend::common_define::{OutputId, WindowId};
use crate::core::models::{ClientKey, MonitorKey, WMClient, WMMonitor};
use slotmap::{SecondaryMap, SlotMap};
use std::collections::HashMap;

pub struct WMState {
    // 核心数据结构
    pub clients: SlotMap<ClientKey, WMClient>,
    pub monitors: SlotMap<MonitorKey, WMMonitor>,

    // 排序与索引
    pub client_order: Vec<ClientKey>,
    pub client_stack_order: Vec<ClientKey>, // 渲染堆叠顺序
    pub monitor_order: Vec<MonitorKey>,
    pub output_map: SecondaryMap<MonitorKey, OutputId>,
    pub win_to_client: HashMap<WindowId, ClientKey>, // WindowId → ClientKey O(1) lookup

    // 焦点与选择
    pub sel_mon: Option<MonitorKey>,
    pub motion_mon: Option<MonitorKey>, // 上次鼠标所在的屏幕

    // 缓存与辅助
    pub monitor_clients: SecondaryMap<MonitorKey, Vec<ClientKey>>,
    pub monitor_stack: SecondaryMap<MonitorKey, Vec<ClientKey>>,
}

impl WMState {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::new(),
            monitors: SlotMap::new(),
            client_order: Vec::new(),
            client_stack_order: Vec::new(),
            monitor_order: Vec::new(),
            output_map: SecondaryMap::new(),
            win_to_client: HashMap::new(),
            sel_mon: None,
            motion_mon: None,
            monitor_clients: SecondaryMap::new(),
            monitor_stack: SecondaryMap::new(),
        }
    }
}
