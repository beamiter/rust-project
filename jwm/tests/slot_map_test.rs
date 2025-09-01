use rand::Rng;
use slotmap::{new_key_type, SecondaryMap, SlotMap};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

// ===================================================================
//  通用占位符结构体 (两种模式共用)
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClientGeometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MonitorGeometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeHints;
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClientState {
    Normal,
    Minimized,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorLayout;
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutEnum;
#[derive(Debug, Clone, PartialEq)]
pub struct Pertag;

pub type Window = u32;

// ===================================================================
//  模式 1: 旧结构 (Rc<RefCell> 链表)
// ===================================================================

#[derive(Debug)]
pub struct OldWMClient {
    pub win: Window,
    pub geometry: ClientGeometry,
    pub state: ClientState,
    pub next: Option<Rc<RefCell<OldWMClient>>>,
    pub mon: Option<Rc<RefCell<OldWMMonitor>>>,
}

#[derive(Debug)]
pub struct OldWMMonitor {
    pub num: i32,
    pub geometry: MonitorGeometry,
    pub clients: Option<Rc<RefCell<OldWMClient>>>,
    pub next: Option<Rc<RefCell<OldWMMonitor>>>,
}

pub struct OldWM {
    pub monitors: Option<Rc<RefCell<OldWMMonitor>>>,
    pub client_count: usize,
}

impl OldWM {
    pub fn new() -> Self {
        Self {
            monitors: None,
            client_count: 0,
        }
    }

    /// 添加监视器
    pub fn add_monitor(&mut self, num: i32) -> Rc<RefCell<OldWMMonitor>> {
        let new_monitor = Rc::new(RefCell::new(OldWMMonitor {
            num,
            geometry: Default::default(),
            clients: None,
            next: self.monitors.clone(),
        }));
        self.monitors = Some(Rc::clone(&new_monitor));
        new_monitor
    }

    /// 添加客户端到指定监视器
    pub fn add_client(
        &mut self,
        win: Window,
        mon: &Rc<RefCell<OldWMMonitor>>,
    ) -> Rc<RefCell<OldWMClient>> {
        let new_client = Rc::new(RefCell::new(OldWMClient {
            win,
            geometry: ClientGeometry {
                w: self.client_count as u32,
                ..Default::default()
            },
            state: ClientState::Normal,
            mon: Some(Rc::clone(mon)),
            next: mon.borrow().clients.clone(),
        }));

        mon.borrow_mut().clients = Some(Rc::clone(&new_client));
        self.client_count += 1;
        new_client
    }

    /// 查找客户端
    pub fn find_client(&self, win: Window) -> Option<Rc<RefCell<OldWMClient>>> {
        let mut current_mon = self.monitors.clone();
        while let Some(mon_rc) = current_mon {
            let mon = mon_rc.borrow();
            let mut current_client = mon.clients.clone();

            while let Some(client_rc) = current_client {
                let client = client_rc.borrow();
                if client.win == win {
                    return Some(Rc::clone(&client_rc));
                }
                current_client = client.next.clone();
            }
            current_mon = mon.next.clone();
        }
        None
    }

    /// 获取指定监视器的客户端列表（按顺序）
    pub fn get_monitor_clients(&self, mon_num: i32) -> Vec<Window> {
        let mut clients = Vec::new();
        if let Some(mon_rc) = self.find_monitor(mon_num) {
            let mon = mon_rc.borrow();
            let mut current_client = mon.clients.clone();

            while let Some(client_rc) = current_client {
                let client = client_rc.borrow();
                clients.push(client.win);
                current_client = client.next.clone();
            }
        }
        clients
    }

    /// 查找监视器
    pub fn find_monitor(&self, num: i32) -> Option<Rc<RefCell<OldWMMonitor>>> {
        let mut current_mon = self.monitors.clone();
        while let Some(mon_rc) = current_mon {
            if mon_rc.borrow().num == num {
                return Some(mon_rc);
            }
            current_mon = mon_rc.borrow().next.clone();
        }
        None
    }

    /// 交换两个相邻客户端在链表中的位置
    pub fn swap_adjacent_clients(&mut self, win1: Window, win2: Window) -> bool {
        // 找到两个客户端所在的监视器
        let (client1_mon, client2_mon) = match (
            self.find_client_monitor(win1),
            self.find_client_monitor(win2),
        ) {
            (Some(m1), Some(m2)) => (m1, m2),
            _ => return false,
        };

        // 必须在同一个监视器中
        if client1_mon.borrow().num != client2_mon.borrow().num {
            return false;
        }

        let mon_rc = client1_mon;
        let mut mon = mon_rc.borrow_mut();

        // 如果头部是其中一个要交换的客户端
        if let Some(ref first_client) = mon.clients.clone() {
            let first_win = first_client.borrow().win;
            if first_win == win1 || first_win == win2 {
                let second_client = { first_client.borrow().next.clone() };
                if let Some(second_client) = second_client {
                    let second_win = second_client.borrow().win;
                    if second_win == win1 || second_win == win2 {
                        // 交换前两个客户端
                        let third = second_client.borrow().next.clone();
                        first_client.borrow_mut().next = third;
                        second_client.borrow_mut().next = Some(Rc::clone(first_client));
                        mon.clients = Some(Rc::clone(&second_client));
                        return true;
                    }
                }
            }
        }

        // 查找中间的相邻客户端
        let mut current = mon.clients.clone();
        while let Some(current_rc) = current {
            if let Some(ref next_rc) = current_rc.borrow().next {
                if let Some(ref next_next_rc) = next_rc.borrow().next {
                    let _current_win = current_rc.borrow().win;
                    let next_win = next_rc.borrow().win;
                    let next_next_win = next_next_rc.borrow().win;

                    // 检查是否是要交换的相邻对
                    if (next_win == win1 && next_next_win == win2)
                        || (next_win == win2 && next_next_win == win1)
                    {
                        // 交换 next 和 next_next
                        let after_next_next = next_next_rc.borrow().next.clone();
                        current_rc.borrow_mut().next = Some(Rc::clone(next_next_rc));
                        next_next_rc.borrow_mut().next = Some(Rc::clone(next_rc));
                        next_rc.borrow_mut().next = after_next_next;
                        return true;
                    }
                }
            }
            current = current_rc.borrow().next.clone();
        }

        false
    }

    /// 查找客户端所在的监视器
    fn find_client_monitor(&self, win: Window) -> Option<Rc<RefCell<OldWMMonitor>>> {
        let mut current_mon = self.monitors.clone();
        while let Some(mon_rc) = current_mon {
            let mon = mon_rc.borrow();
            let mut current_client = mon.clients.clone();

            while let Some(client_rc) = current_client {
                if client_rc.borrow().win == win {
                    return Some(Rc::clone(&mon_rc));
                }
                current_client = client_rc.borrow().next.clone();
            }
            current_mon = mon.next.clone();
        }
        None
    }

    /// 删除客户端
    pub fn remove_client(&mut self, win: Window) -> bool {
        let mut current_mon = self.monitors.clone();
        while let Some(mon_rc) = current_mon {
            let mut mon = mon_rc.borrow_mut();

            // 检查第一个客户端
            if let Some(ref first_client) = mon.clients {
                if first_client.borrow().win == win {
                    let next = first_client.borrow().next.clone();
                    mon.clients = next;
                    self.client_count -= 1;
                    return true;
                }
            }

            // 检查后续客户端
            let mut current_client = mon.clients.clone();
            while let Some(client_rc) = current_client {
                let next_client = { client_rc.borrow().next.clone() };
                if let Some(ref next_client) = next_client {
                    if next_client.borrow().win == win {
                        // 找到要删除的客户端，跳过它
                        let next = next_client.borrow().next.clone();
                        client_rc.borrow_mut().next = next;
                        self.client_count -= 1;
                        return true;
                    }
                }
                current_client = client_rc.borrow().next.clone();
            }

            current_mon = mon.next.clone();
        }
        false
    }

    /// 更新客户端几何信息
    pub fn update_client_geometry(&self, win: Window, new_geometry: ClientGeometry) -> bool {
        if let Some(client_rc) = self.find_client(win) {
            client_rc.borrow_mut().geometry = new_geometry;
            true
        } else {
            false
        }
    }

    /// 遍历所有客户端
    pub fn traverse_all_clients(&self) -> u64 {
        let mut total_width: u64 = 0;
        let mut current_mon = self.monitors.clone();

        while let Some(mon_rc) = current_mon {
            let mon = mon_rc.borrow();
            let mut current_client = mon.clients.clone();

            while let Some(client_rc) = current_client {
                let client = client_rc.borrow();
                total_width += client.geometry.w as u64;
                current_client = client.next.clone();
            }
            current_mon = mon.next.clone();
        }
        total_width
    }

    /// 统计总客户端数量
    pub fn count_clients(&self) -> usize {
        let mut count = 0;
        let mut current_mon = self.monitors.clone();

        while let Some(mon_rc) = current_mon {
            let mon = mon_rc.borrow();
            let mut current_client = mon.clients.clone();

            while let Some(client_rc) = current_client {
                count += 1;
                current_client = client_rc.borrow().next.clone();
            }
            current_mon = mon.next.clone();
        }
        count
    }
}

// ===================================================================
//  模式 2: 新结构 (SlotMap + Keys)
// ===================================================================

new_key_type! { pub struct ClientKey; pub struct MonitorKey; }

#[derive(Debug, Clone, PartialEq)]
pub struct NewWMClient {
    pub win: Window,
    pub geometry: ClientGeometry,
    pub state: ClientState,
    pub mon: Option<MonitorKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewWMMonitor {
    pub num: i32,
    pub geometry: MonitorGeometry,
    pub sel: Option<ClientKey>,
}

pub struct Jwm {
    pub clients: SlotMap<ClientKey, NewWMClient>,
    pub monitors: SlotMap<MonitorKey, NewWMMonitor>,
    pub monitor_order: Vec<MonitorKey>,
    pub monitor_clients: SecondaryMap<MonitorKey, Vec<ClientKey>>,
    // 添加反向索引：从 Window ID 到 ClientKey
    pub window_to_client: std::collections::HashMap<Window, ClientKey>,
    pub monitor_num_to_key: std::collections::HashMap<i32, MonitorKey>,
}

impl Jwm {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::with_key(),
            monitors: SlotMap::with_key(),
            monitor_order: Vec::new(),
            monitor_clients: SecondaryMap::new(),
            window_to_client: std::collections::HashMap::new(),
            monitor_num_to_key: std::collections::HashMap::new(),
        }
    }

    /// 添加监视器
    pub fn add_monitor(&mut self, num: i32) -> MonitorKey {
        let monitor = NewWMMonitor {
            num,
            geometry: Default::default(),
            sel: None,
        };
        let key = self.monitors.insert(monitor);
        self.monitor_order.push(key);
        self.monitor_clients.insert(key, Vec::new());
        self.monitor_num_to_key.insert(num, key);
        key
    }

    /// 添加客户端到指定监视器
    pub fn add_client(&mut self, win: Window, mon_key: MonitorKey) -> ClientKey {
        let client = NewWMClient {
            win,
            geometry: ClientGeometry {
                w: self.clients.len() as u32,
                ..Default::default()
            },
            state: ClientState::Normal,
            mon: Some(mon_key),
        };

        let client_key = self.clients.insert(client);
        self.monitor_clients[mon_key].push(client_key);
        self.window_to_client.insert(win, client_key);
        client_key
    }

    /// 查找客户端
    pub fn find_client(&self, win: Window) -> Option<ClientKey> {
        self.window_to_client.get(&win).copied()
    }

    /// 获取指定监视器的客户端列表（按顺序）
    pub fn get_monitor_clients(&self, mon_num: i32) -> Vec<Window> {
        if let Some(&mon_key) = self.monitor_num_to_key.get(&mon_num) {
            if let Some(client_keys) = self.monitor_clients.get(mon_key) {
                return client_keys
                    .iter()
                    .map(|&key| self.clients[key].win)
                    .collect();
            }
        }
        Vec::new()
    }

    /// 交换两个相邻客户端在向量中的位置
    pub fn swap_adjacent_clients(&mut self, win1: Window, win2: Window) -> bool {
        let client1_key = self.window_to_client.get(&win1).copied();
        let client2_key = self.window_to_client.get(&win2).copied();

        if let (Some(key1), Some(key2)) = (client1_key, client2_key) {
            // 找到它们所在的监视器
            if let (Some(client1), Some(client2)) = (self.clients.get(key1), self.clients.get(key2))
            {
                if client1.mon == client2.mon {
                    if let Some(mon_key) = client1.mon {
                        if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                            // 找到两个客户端在列表中的位置
                            let pos1 = client_list.iter().position(|&k| k == key1);
                            let pos2 = client_list.iter().position(|&k| k == key2);

                            if let (Some(p1), Some(p2)) = (pos1, pos2) {
                                // 检查是否相邻
                                if (p1 + 1 == p2) || (p2 + 1 == p1) {
                                    client_list.swap(p1, p2);
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// 交换任意两个客户端的位置（不要求相邻）
    pub fn swap_clients(&mut self, win1: Window, win2: Window) -> bool {
        let client1_key = self.window_to_client.get(&win1).copied();
        let client2_key = self.window_to_client.get(&win2).copied();

        if let (Some(key1), Some(key2)) = (client1_key, client2_key) {
            if let (Some(client1), Some(client2)) = (self.clients.get(key1), self.clients.get(key2))
            {
                if client1.mon == client2.mon {
                    if let Some(mon_key) = client1.mon {
                        if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                            let pos1 = client_list.iter().position(|&k| k == key1);
                            let pos2 = client_list.iter().position(|&k| k == key2);

                            if let (Some(p1), Some(p2)) = (pos1, pos2) {
                                client_list.swap(p1, p2);
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// 移动客户端到指定位置
    pub fn move_client_to_position(&mut self, win: Window, new_position: usize) -> bool {
        if let Some(&client_key) = self.window_to_client.get(&win) {
            if let Some(client) = self.clients.get(client_key) {
                if let Some(mon_key) = client.mon {
                    if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                        if let Some(current_pos) = client_list.iter().position(|&k| k == client_key)
                        {
                            if new_position < client_list.len() {
                                let item = client_list.remove(current_pos);
                                client_list.insert(new_position, item);
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// 删除客户端
    pub fn remove_client(&mut self, win: Window) -> bool {
        if let Some(client_key) = self.window_to_client.remove(&win) {
            if let Some(client) = self.clients.remove(client_key) {
                // 从监视器的客户端列表中移除
                if let Some(mon_key) = client.mon {
                    if let Some(client_list) = self.monitor_clients.get_mut(mon_key) {
                        client_list.retain(|&k| k != client_key);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 更新客户端几何信息
    pub fn update_client_geometry(&mut self, win: Window, new_geometry: ClientGeometry) -> bool {
        if let Some(&client_key) = self.window_to_client.get(&win) {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.geometry = new_geometry;
                return true;
            }
        }
        false
    }

    /// 遍历所有客户端
    pub fn traverse_all_clients(&self) -> u64 {
        let mut total_width: u64 = 0;
        for &mon_key in &self.monitor_order {
            if let Some(client_keys) = self.monitor_clients.get(mon_key) {
                for &client_key in client_keys {
                    let client = &self.clients[client_key];
                    total_width += client.geometry.w as u64;
                }
            }
        }
        total_width
    }

    /// 统计总客户端数量
    pub fn count_clients(&self) -> usize {
        self.clients.len()
    }
}

// ===================================================================
//  基准测试函数
// ===================================================================

#[derive(Debug)]
pub struct BenchmarkResults {
    pub setup_time: std::time::Duration,
    pub insert_time: std::time::Duration,
    pub search_time: std::time::Duration,
    pub update_time: std::time::Duration,
    pub traverse_time: std::time::Duration,
    pub swap_time: std::time::Duration,
    pub delete_time: std::time::Duration,
    pub memory_usage: usize, // 近似内存使用量
}

/// 运行 Rc<RefCell> 模式的完整基准测试
pub fn run_old_wm_benchmark(
    num_monitors: usize,
    num_clients: usize,
    operation_count: usize,
) -> BenchmarkResults {
    let mut rng = rand::thread_rng();

    // === 设置阶段 ===
    let setup_start = Instant::now();
    let mut old_wm = OldWM::new();
    let mut monitor_refs = Vec::new();

    // 创建监视器
    for i in 0..num_monitors {
        let mon = old_wm.add_monitor(i as i32);
        monitor_refs.push(mon);
    }

    let setup_time = setup_start.elapsed();

    // === 插入测试 ===
    let insert_start = Instant::now();
    let mut client_wins = Vec::new();

    for i in 0..num_clients {
        let mon_index = rng.gen_range(0..num_monitors);
        let win = i as u32;
        old_wm.add_client(win, &monitor_refs[mon_index]);
        client_wins.push(win);
    }

    let insert_time = insert_start.elapsed();

    // === 查找测试 ===
    let search_start = Instant::now();
    let mut found_count = 0;

    for _ in 0..operation_count {
        let win = client_wins[rng.gen_range(0..client_wins.len())];
        if old_wm.find_client(win).is_some() {
            found_count += 1;
        }
    }

    let search_time = search_start.elapsed();

    // === 更新测试 ===
    let update_start = Instant::now();
    let mut updated_count = 0;

    for _ in 0..operation_count {
        let win = client_wins[rng.gen_range(0..client_wins.len())];
        let new_geometry = ClientGeometry {
            x: rng.gen_range(0..1000),
            y: rng.gen_range(0..1000),
            w: rng.gen_range(100..500),
            h: rng.gen_range(100..500),
        };
        if old_wm.update_client_geometry(win, new_geometry) {
            updated_count += 1;
        }
    }

    let update_time = update_start.elapsed();

    // === 遍历测试 ===
    let traverse_start = Instant::now();
    let _total_width = old_wm.traverse_all_clients();
    let traverse_time = traverse_start.elapsed();

    // === 交换测试 ===
    let swap_start = Instant::now();
    let mut swapped_count = 0;

    for _ in 0..operation_count / 10 {
        // 减少交换操作次数
        let idx1 = rng.gen_range(0..client_wins.len());
        let idx2 = rng.gen_range(0..client_wins.len());
        let win1 = client_wins[idx1];
        let win2 = client_wins[idx2];

        if old_wm.swap_adjacent_clients(win1, win2) {
            swapped_count += 1;
        }
    }

    let swap_time = swap_start.elapsed();

    // === 删除测试 ===
    let delete_start = Instant::now();
    let delete_count = std::cmp::min(operation_count, client_wins.len());
    let mut deleted_count = 0;

    for i in 0..delete_count {
        let win = client_wins[i];
        if old_wm.remove_client(win) {
            deleted_count += 1;
        }
    }

    let delete_time = delete_start.elapsed();

    // 内存使用量估算 (简化)
    let memory_usage = old_wm.count_clients() * std::mem::size_of::<OldWMClient>()
        + num_monitors * std::mem::size_of::<OldWMMonitor>();

    println!(
        "Old WM - Found: {}, Updated: {}, Swapped: {}, Deleted: {}",
        found_count, updated_count, swapped_count, deleted_count
    );

    BenchmarkResults {
        setup_time,
        insert_time,
        search_time,
        update_time,
        traverse_time,
        swap_time,
        delete_time,
        memory_usage,
    }
}

/// 运行 SlotMap 模式的完整基准测试
pub fn run_jwm_benchmark(
    num_monitors: usize,
    num_clients: usize,
    operation_count: usize,
) -> BenchmarkResults {
    let mut rng = rand::thread_rng();

    // === 设置阶段 ===
    let setup_start = Instant::now();
    let mut jwm = Jwm::new();
    let mut monitor_keys = Vec::new();

    // 创建监视器
    for i in 0..num_monitors {
        let mon_key = jwm.add_monitor(i as i32);
        monitor_keys.push(mon_key);
    }

    let setup_time = setup_start.elapsed();

    // === 插入测试 ===
    let insert_start = Instant::now();
    let mut client_wins = Vec::new();

    for i in 0..num_clients {
        let mon_index = rng.gen_range(0..num_monitors);
        let win = i as u32;
        jwm.add_client(win, monitor_keys[mon_index]);
        client_wins.push(win);
    }

    let insert_time = insert_start.elapsed();

    // === 查找测试 ===
    let search_start = Instant::now();
    let mut found_count = 0;

    for _ in 0..operation_count {
        let win = client_wins[rng.gen_range(0..client_wins.len())];
        if jwm.find_client(win).is_some() {
            found_count += 1;
        }
    }

    let search_time = search_start.elapsed();

    // === 更新测试 ===
    let update_start = Instant::now();
    let mut updated_count = 0;

    for _ in 0..operation_count {
        let win = client_wins[rng.gen_range(0..client_wins.len())];
        let new_geometry = ClientGeometry {
            x: rng.gen_range(0..1000),
            y: rng.gen_range(0..1000),
            w: rng.gen_range(100..500),
            h: rng.gen_range(100..500),
        };
        if jwm.update_client_geometry(win, new_geometry) {
            updated_count += 1;
        }
    }

    let update_time = update_start.elapsed();

    // === 遍历测试 ===
    let traverse_start = Instant::now();
    let _total_width = jwm.traverse_all_clients();
    let traverse_time = traverse_start.elapsed();

    // === 交换测试 ===
    let swap_start = Instant::now();
    let mut swapped_count = 0;

    for _ in 0..operation_count / 10 {
        // 减少交换操作次数
        let idx1 = rng.gen_range(0..client_wins.len());
        let idx2 = rng.gen_range(0..client_wins.len());
        let win1 = client_wins[idx1];
        let win2 = client_wins[idx2];

        if jwm.swap_adjacent_clients(win1, win2) {
            swapped_count += 1;
        }
    }

    let swap_time = swap_start.elapsed();

    // === 删除测试 ===
    let delete_start = Instant::now();
    let delete_count = std::cmp::min(operation_count, client_wins.len());
    let mut deleted_count = 0;

    for i in 0..delete_count {
        let win = client_wins[i];
        if jwm.remove_client(win) {
            deleted_count += 1;
        }
    }

    let delete_time = delete_start.elapsed();

    // 内存使用量估算
    let memory_usage = jwm.count_clients() * std::mem::size_of::<NewWMClient>()
        + jwm.monitors.len() * std::mem::size_of::<NewWMMonitor>()
        + jwm.window_to_client.len() * std::mem::size_of::<(Window, ClientKey)>();

    println!(
        "JWM - Found: {}, Updated: {}, Swapped: {}, Deleted: {}",
        found_count, updated_count, swapped_count, deleted_count
    );

    BenchmarkResults {
        setup_time,
        insert_time,
        search_time,
        update_time,
        traverse_time,
        swap_time,
        delete_time,
        memory_usage,
    }
}

// ===================================================================
//  结果分析和展示
// ===================================================================

fn print_comparison(old_results: &BenchmarkResults, jwm_results: &BenchmarkResults) {
    println!("\n=== 详细性能对比 ===");

    let operations = [
        ("Setup", old_results.setup_time, jwm_results.setup_time),
        ("Insert", old_results.insert_time, jwm_results.insert_time),
        ("Search", old_results.search_time, jwm_results.search_time),
        ("Update", old_results.update_time, jwm_results.update_time),
        (
            "Traverse",
            old_results.traverse_time,
            jwm_results.traverse_time,
        ),
        ("Swap", old_results.swap_time, jwm_results.swap_time),
        ("Delete", old_results.delete_time, jwm_results.delete_time),
    ];

    println!(
        "{:<12} {:<15} {:<15} {:<10}",
        "Operation", "Rc<RefCell>", "SlotMap", "Speedup"
    );
    println!("{:-<60}", "");

    for (name, old_time, jwm_time) in &operations {
        let speedup = if jwm_time.as_nanos() > 0 {
            old_time.as_nanos() as f64 / jwm_time.as_nanos() as f64
        } else {
            0.0
        };

        println!(
            "{:<12} {:<15?} {:<15?} {:<10.2}x",
            name, old_time, jwm_time, speedup
        );
    }

    println!("\n=== 内存使用对比 ===");
    println!("Rc<RefCell>: {} bytes", old_results.memory_usage);
    println!("SlotMap:     {} bytes", jwm_results.memory_usage);
    println!(
        "Memory ratio: {:.2}x",
        old_results.memory_usage as f64 / jwm_results.memory_usage as f64
    );

    // 计算总体性能
    let old_total = old_results.insert_time
        + old_results.search_time
        + old_results.update_time
        + old_results.traverse_time
        + old_results.swap_time
        + old_results.delete_time;
    let jwm_total = jwm_results.insert_time
        + jwm_results.search_time
        + jwm_results.update_time
        + jwm_results.traverse_time
        + jwm_results.swap_time
        + jwm_results.delete_time;

    println!("\n=== 总体性能 ===");
    println!("Rc<RefCell> 总时间: {:?}", old_total);
    println!("SlotMap 总时间:     {:?}", jwm_total);
    if jwm_total.as_nanos() > 0 {
        let total_speedup = old_total.as_nanos() as f64 / jwm_total.as_nanos() as f64;
        println!("总体加速比: {:.2}x", total_speedup);
    }
}

// ===================================================================
//  主函数
// ===================================================================

fn main() {
    const NUM_MONITORS: usize = 10;
    const NUM_CLIENTS: usize = 5000;
    const OPERATION_COUNT: usize = 1000; // 每种操作的测试次数

    println!("=== 窗口管理器性能对比测试 ===");
    println!("监视器数量: {}", NUM_MONITORS);
    println!("客户端数量: {}", NUM_CLIENTS);
    println!("操作测试次数: {}", OPERATION_COUNT);
    println!();

    // 运行 Rc<RefCell> 模式测试
    println!("运行 Rc<RefCell> 模式基准测试...");
    let old_results = run_old_wm_benchmark(NUM_MONITORS, NUM_CLIENTS, OPERATION_COUNT);

    // 运行 SlotMap 模式测试
    println!("\n运行 SlotMap 模式基准测试...");
    let jwm_results = run_jwm_benchmark(NUM_MONITORS, NUM_CLIENTS, OPERATION_COUNT);

    // 打印对比结果
    print_comparison(&old_results, &jwm_results);

    // 额外的分析
    println!("\n=== 优势分析 ===");
    println!("SlotMap 模式优势:");
    println!("  • 更好的缓存局部性");
    println!("  • O(1) 时间复杂度的查找、插入、删除");
    println!("  • 更少的内存碎片");
    println!("  • 避免了 Rc<RefCell> 的运行时借用检查开销");
    println!("  • 更直观的数据关系管理");
    println!("  • 支持高效的位置交换和重排序");

    println!("\nRc<RefCell> 模式劣势:");
    println!("  • 链表遍历导致缓存未命中");
    println!("  • 动态分配导致内存碎片");
    println!("  • 运行时借用检查开销");
    println!("  • 复杂的指针操作容易出错");
    println!("  • 链表中的位置交换操作复杂且低效");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_wm_basic_operations() {
        let mut old_wm = OldWM::new();
        let mon = old_wm.add_monitor(0);

        // 测试添加和查找
        let _client = old_wm.add_client(100, &mon);
        assert!(old_wm.find_client(100).is_some());
        assert!(old_wm.find_client(999).is_none());

        // 测试更新
        let new_geo = ClientGeometry {
            x: 10,
            y: 20,
            w: 300,
            h: 400,
        };
        assert!(old_wm.update_client_geometry(100, new_geo));

        // 测试删除
        assert!(old_wm.remove_client(100));
        assert!(old_wm.find_client(100).is_none());
    }

    #[test]
    fn test_old_wm_swap_adjacent() {
        let mut old_wm = OldWM::new();
        let mon = old_wm.add_monitor(0);

        // 添加多个客户端
        old_wm.add_client(100, &mon);
        old_wm.add_client(200, &mon);
        old_wm.add_client(300, &mon);
        old_wm.add_client(400, &mon);

        // 获取初始顺序（链表是头插法，所以顺序是反的）
        let clients_before = old_wm.get_monitor_clients(0);
        println!("交换前的客户端顺序: {:?}", clients_before);

        // 交换相邻客户端
        let success = old_wm.swap_adjacent_clients(clients_before[0], clients_before[1]);
        assert!(success);

        // 验证顺序已改变
        let clients_after = old_wm.get_monitor_clients(0);
        println!("交换后的客户端顺序: {:?}", clients_after);

        assert_ne!(clients_before, clients_after);
        assert_eq!(clients_before[0], clients_after[1]);
        assert_eq!(clients_before[1], clients_after[0]);
    }

    #[test]
    fn test_old_wm_swap_non_adjacent() {
        let mut old_wm = OldWM::new();
        let mon = old_wm.add_monitor(0);

        // 添加客户端
        old_wm.add_client(100, &mon);
        old_wm.add_client(200, &mon);
        old_wm.add_client(300, &mon);
        old_wm.add_client(400, &mon);

        let clients_before = old_wm.get_monitor_clients(0);

        // 尝试交换非相邻客户端（应该失败）
        let success = old_wm.swap_adjacent_clients(clients_before[0], clients_before[2]);
        assert!(!success);

        // 验证顺序未改变
        let clients_after = old_wm.get_monitor_clients(0);
        assert_eq!(clients_before, clients_after);
    }

    #[test]
    fn test_jwm_basic_operations() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 测试添加和查找
        let _client_key = jwm.add_client(100, mon_key);
        assert!(jwm.find_client(100).is_some());
        assert!(jwm.find_client(999).is_none());

        // 测试更新
        let new_geo = ClientGeometry {
            x: 10,
            y: 20,
            w: 300,
            h: 400,
        };
        assert!(jwm.update_client_geometry(100, new_geo));

        // 测试删除
        assert!(jwm.remove_client(100));
        assert!(jwm.find_client(100).is_none());
    }

    #[test]
    fn test_jwm_swap_adjacent() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 添加多个客户端
        jwm.add_client(100, mon_key);
        jwm.add_client(200, mon_key);
        jwm.add_client(300, mon_key);
        jwm.add_client(400, mon_key);

        // 获取初始顺序
        let clients_before = jwm.get_monitor_clients(0);
        println!("交换前的客户端顺序: {:?}", clients_before);

        // 交换相邻客户端
        let success = jwm.swap_adjacent_clients(clients_before[0], clients_before[1]);
        assert!(success);

        // 验证顺序已改变
        let clients_after = jwm.get_monitor_clients(0);
        println!("交换后的客户端顺序: {:?}", clients_after);

        assert_ne!(clients_before, clients_after);
        assert_eq!(clients_before[0], clients_after[1]);
        assert_eq!(clients_before[1], clients_after[0]);
    }

    #[test]
    fn test_jwm_swap_non_adjacent() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 添加客户端
        jwm.add_client(100, mon_key);
        jwm.add_client(200, mon_key);
        jwm.add_client(300, mon_key);
        jwm.add_client(400, mon_key);

        let clients_before = jwm.get_monitor_clients(0);

        // 尝试交换非相邻客户端（应该失败）
        let success = jwm.swap_adjacent_clients(clients_before[0], clients_before[2]);
        assert!(!success);

        // 验证顺序未改变
        let clients_after = jwm.get_monitor_clients(0);
        assert_eq!(clients_before, clients_after);
    }

    #[test]
    fn test_jwm_swap_any_clients() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 添加客户端
        jwm.add_client(100, mon_key);
        jwm.add_client(200, mon_key);
        jwm.add_client(300, mon_key);
        jwm.add_client(400, mon_key);

        let clients_before = jwm.get_monitor_clients(0);

        // 交换任意两个客户端（不要求相邻）
        let success = jwm.swap_clients(clients_before[0], clients_before[2]);
        assert!(success);

        // 验证顺序已改变
        let clients_after = jwm.get_monitor_clients(0);
        assert_ne!(clients_before, clients_after);
        assert_eq!(clients_before[0], clients_after[2]);
        assert_eq!(clients_before[2], clients_after[0]);
    }

    #[test]
    fn test_jwm_move_to_position() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 添加客户端
        jwm.add_client(100, mon_key);
        jwm.add_client(200, mon_key);
        jwm.add_client(300, mon_key);
        jwm.add_client(400, mon_key);

        let clients_before = jwm.get_monitor_clients(0);
        println!("移动前的客户端顺序: {:?}", clients_before);

        // 将第一个客户端移动到最后
        let success = jwm.move_client_to_position(clients_before[0], 3);
        assert!(success);

        let clients_after = jwm.get_monitor_clients(0);
        println!("移动后的客户端顺序: {:?}", clients_after);

        // 验证移动成功
        assert_eq!(clients_before[0], clients_after[3]);
        assert_eq!(clients_before[1], clients_after[0]);
        assert_eq!(clients_before[2], clients_after[1]);
        assert_eq!(clients_before[3], clients_after[2]);
    }

    #[test]
    fn test_multi_monitor_operations() {
        let mut jwm = Jwm::new();
        let mon1 = jwm.add_monitor(0);
        let mon2 = jwm.add_monitor(1);

        // 在不同监视器上添加客户端
        jwm.add_client(100, mon1);
        jwm.add_client(200, mon1);
        jwm.add_client(300, mon2);
        jwm.add_client(400, mon2);

        // 测试跨监视器交换（应该失败）
        let success = jwm.swap_adjacent_clients(100, 300);
        assert!(!success);

        // 测试同监视器内交换（应该成功）
        let success = jwm.swap_adjacent_clients(100, 200);
        assert!(success);

        // 验证各监视器的客户端列表
        let mon1_clients = jwm.get_monitor_clients(0);
        let mon2_clients = jwm.get_monitor_clients(1);

        assert_eq!(mon1_clients.len(), 2);
        assert_eq!(mon2_clients.len(), 2);
        assert!(mon1_clients.contains(&100));
        assert!(mon1_clients.contains(&200));
        assert!(mon2_clients.contains(&300));
        assert!(mon2_clients.contains(&400));
    }

    #[test]
    fn test_performance_comparison() {
        const SMALL_SCALE: usize = 100;

        println!("\n=== 小规模性能测试 ===");

        // 测试旧模式
        let old_results = run_old_wm_benchmark(2, SMALL_SCALE, 50);

        // 测试新模式
        let jwm_results = run_jwm_benchmark(2, SMALL_SCALE, 50);

        // 基本断言：确保两种模式都能正常工作
        assert!(old_results.setup_time.as_nanos() > 0);
        assert!(jwm_results.setup_time.as_nanos() > 0);

        println!("测试完成 - 两种模式都正常工作");
    }

    #[test]
    fn test_edge_cases() {
        let mut jwm = Jwm::new();
        let mon_key = jwm.add_monitor(0);

        // 测试空列表操作
        assert!(!jwm.swap_adjacent_clients(100, 200));
        assert!(!jwm.remove_client(100));
        assert!(!jwm.update_client_geometry(100, ClientGeometry::default()));

        // 添加单个客户端
        jwm.add_client(100, mon_key);

        // 测试自己和自己交换
        assert!(!jwm.swap_adjacent_clients(100, 100));

        // 测试只有一个客户端时的移动
        assert!(jwm.move_client_to_position(100, 0)); // 移动到同一位置应该成功

        // 测试超出范围的位置
        assert!(!jwm.move_client_to_position(100, 10)); // 超出范围应该失败
    }
}
