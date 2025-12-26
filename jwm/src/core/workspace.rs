// src/core/workspace.rs

use crate::core::models::WMMonitor;

pub struct WorkspaceManager;

impl WorkspaceManager {
    /// 计算 client.state.tags 的变更
    pub fn calculate_new_tags(current_tags: u32, mask: u32, toggle: bool) -> u32 {
        if toggle { current_tags ^ mask } else { mask }
    }

    /// 检查 target_tag 是否已经是当前 Monitor 的激活 Tag
    pub fn is_same_tag(monitor: &WMMonitor, target_tag_mask: u32) -> bool {
        monitor.tag_set[monitor.sel_tags] == target_tag_mask
    }
}
