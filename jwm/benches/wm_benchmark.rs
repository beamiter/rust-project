use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::Rng;
use slotmap::{new_key_type, SecondaryMap, SlotMap};
use std::cell::RefCell;
use std::rc::Rc;

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
    Fullscreen,
}

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
}

impl OldWM {
    pub fn new() -> Self {
        Self { monitors: None }
    }

    /// 添加监视器
    pub fn add_monitor(&mut self, num: i32) {
        let new_monitor = Rc::new(RefCell::new(OldWMMonitor {
            num,
            geometry: Default::default(),
            clients: None,
            next: self.monitors.clone(),
        }));
        self.monitors = Some(new_monitor);
    }

    /// 添加客户端到指定监视器
    pub fn add_client(&mut self, win: Window, monitor_num: i32) -> bool {
        if let Some(monitor) = self.find_monitor(monitor_num) {
            let new_client = Rc::new(RefCell::new(OldWMClient {
                win,
                geometry: ClientGeometry {
                    w: win,
                    ..Default::default()
                },
                state: ClientState::Normal,
                next: monitor.borrow().clients.clone(),
                mon: Some(Rc::clone(&monitor)),
            }));
            monitor.borrow_mut().clients = Some(new_client);
            true
        } else {
            false
        }
    }

    /// 查找监视器
    pub fn find_monitor(&self, num: i32) -> Option<Rc<RefCell<OldWMMonitor>>> {
        let mut current = self.monitors.clone();
        while let Some(mon) = current {
            if mon.borrow().num == num {
                return Some(mon);
            }
            current = mon.borrow().next.clone();
        }
        None
    }

    /// 查找客户端
    pub fn find_client(&self, win: Window) -> Option<Rc<RefCell<OldWMClient>>> {
        let mut current_mon = self.monitors.clone();
        while let Some(mon) = current_mon {
            let mut current_client = mon.borrow().clients.clone();
            while let Some(client) = current_client {
                if client.borrow().win == win {
                    return Some(client);
                }
                current_client = client.borrow().next.clone();
            }
            current_mon = mon.borrow().next.clone();
        }
        None
    }

    /// 删除客户端
    pub fn remove_client(&mut self, win: Window) -> bool {
        let mut current_mon = self.monitors.clone();
        while let Some(mon) = current_mon {
            let mut prev_client: Option<Rc<RefCell<OldWMClient>>> = None;
            let mut current_client = mon.borrow().clients.clone();

            while let Some(client) = current_client {
                if client.borrow().win == win {
                    let next = client.borrow().next.clone();
                    if let Some(prev) = prev_client {
                        prev.borrow_mut().next = next;
                    } else {
                        mon.borrow_mut().clients = next;
                    }
                    return true;
                }
                prev_client = Some(Rc::clone(&client));
                current_client = client.borrow().next.clone();
            }
            current_mon = mon.borrow().next.clone();
        }
        false
    }

    /// 更新客户端几何信息
    pub fn update_client_geometry(&mut self, win: Window, geometry: ClientGeometry) -> bool {
        if let Some(client) = self.find_client(win) {
            client.borrow_mut().geometry = geometry;
            true
        } else {
            false
        }
    }

    /// 交换相邻的两个客户端的几何信息
    pub fn swap_adjacent_clients(&mut self, win1: Window, win2: Window) -> bool {
        if let (Some(client1), Some(client2)) = (self.find_client(win1), self.find_client(win2)) {
            let geom1 = client1.borrow().geometry;
            let geom2 = client2.borrow().geometry;
            client1.borrow_mut().geometry = geom2;
            client2.borrow_mut().geometry = geom1;
            true
        } else {
            false
        }
    }

    /// 遍历所有客户端
    pub fn traverse_all_clients(&self) -> u64 {
        let mut total_width = 0u64;
        let mut current_mon = self.monitors.clone();

        while let Some(mon) = current_mon {
            let mut current_client = mon.borrow().clients.clone();
            while let Some(client) = current_client {
                total_width += client.borrow().geometry.w as u64;
                current_client = client.borrow().next.clone();
            }
            current_mon = mon.borrow().next.clone();
        }
        total_width
    }
}

// ===================================================================
//  模式 2: 新结构 (SlotMap + Keys)
// ===================================================================

new_key_type! {
    pub struct ClientKey;
    pub struct MonitorKey;
}

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

pub struct NewWM {
    pub clients: SlotMap<ClientKey, NewWMClient>,
    pub monitors: SlotMap<MonitorKey, NewWMMonitor>,
    pub monitor_order: Vec<MonitorKey>,
    pub monitor_clients: SecondaryMap<MonitorKey, Vec<ClientKey>>,
    // 添加反向查找映射
    pub client_to_key: std::collections::HashMap<Window, ClientKey>,
    pub monitor_num_to_key: std::collections::HashMap<i32, MonitorKey>,
}

impl NewWM {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::with_key(),
            monitors: SlotMap::with_key(),
            monitor_order: Vec::new(),
            monitor_clients: SecondaryMap::new(),
            client_to_key: std::collections::HashMap::new(),
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
    pub fn add_client(&mut self, win: Window, monitor_num: i32) -> Option<ClientKey> {
        if let Some(&monitor_key) = self.monitor_num_to_key.get(&monitor_num) {
            let client = NewWMClient {
                win,
                geometry: ClientGeometry {
                    w: win,
                    ..Default::default()
                },
                state: ClientState::Normal,
                mon: Some(monitor_key),
            };
            let client_key = self.clients.insert(client);
            self.monitor_clients[monitor_key].push(client_key);
            self.client_to_key.insert(win, client_key);
            Some(client_key)
        } else {
            None
        }
    }

    /// 查找客户端
    pub fn find_client(&self, win: Window) -> Option<&NewWMClient> {
        self.client_to_key
            .get(&win)
            .and_then(|&key| self.clients.get(key))
    }

    /// 删除客户端
    pub fn remove_client(&mut self, win: Window) -> bool {
        if let Some(client_key) = self.client_to_key.remove(&win) {
            if let Some(client) = self.clients.remove(client_key) {
                // 从监视器的客户端列表中移除
                if let Some(monitor_key) = client.mon {
                    if let Some(client_list) = self.monitor_clients.get_mut(monitor_key) {
                        client_list.retain(|&key| key != client_key);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 更新客户端几何信息
    pub fn update_client_geometry(&mut self, win: Window, geometry: ClientGeometry) -> bool {
        if let Some(&client_key) = self.client_to_key.get(&win) {
            if let Some(client) = self.clients.get_mut(client_key) {
                client.geometry = geometry;
                return true;
            }
        }
        false
    }

    /// 交换相邻的两个客户端的几何信息
    /// 修复借用检查器问题的方法
    pub fn swap_adjacent_clients(&mut self, win1: Window, win2: Window) -> bool {
        // 方法1: 先获取key，然后检查它们是否有效
        let key1 = self.client_to_key.get(&win1).copied();
        let key2 = self.client_to_key.get(&win2).copied();

        match (key1, key2) {
            (Some(k1), Some(k2)) if k1 != k2 => {
                // 先读取两个几何信息
                let geom1 = self.clients.get(k1).unwrap().geometry;
                let geom2 = self.clients.get(k2).unwrap().geometry;

                // 然后分别更新
                if let Some(client1) = self.clients.get_mut(k1) {
                    client1.geometry = geom2;
                }
                if let Some(client2) = self.clients.get_mut(k2) {
                    client2.geometry = geom1;
                }
                true
            }
            _ => false,
        }
    }

    /// 替代的交换实现，使用更安全的方式
    pub fn swap_adjacent_clients_safe(&mut self, win1: Window, win2: Window) -> bool {
        if let (Some(&key1), Some(&key2)) =
            (self.client_to_key.get(&win1), self.client_to_key.get(&win2))
        {
            if key1 == key2 {
                return false; // 不能交换自己
            }

            // 使用 SlotMap 的安全接口
            if self.clients.contains_key(key1) && self.clients.contains_key(key2) {
                // 临时存储第一个客户端的几何信息
                let temp_geom = self.clients[key1].geometry;

                // 将第二个客户端的几何信息复制到第一个
                self.clients[key1].geometry = self.clients[key2].geometry;

                // 将临时存储的几何信息设置到第二个客户端
                self.clients[key2].geometry = temp_geom;

                return true;
            }
        }
        false
    }

    /// 遍历所有客户端
    pub fn traverse_all_clients(&self) -> u64 {
        let mut total_width = 0u64;
        for &monitor_key in &self.monitor_order {
            if let Some(client_keys) = self.monitor_clients.get(monitor_key) {
                for &client_key in client_keys {
                    total_width += self.clients[client_key].geometry.w as u64;
                }
            }
        }
        total_width
    }
}

// ===================================================================
//  基准测试函数
// ===================================================================

/// 设置旧模式测试数据
pub fn setup_old_wm(num_monitors: usize, num_clients: usize) -> OldWM {
    let mut wm = OldWM::new();
    let mut rng = rand::thread_rng();

    // 添加监视器
    for i in 0..num_monitors {
        wm.add_monitor(i as i32);
    }

    // 添加客户端
    for i in 0..num_clients {
        let monitor_num = rng.gen_range(0..num_monitors) as i32;
        wm.add_client(i as u32, monitor_num);
    }

    wm
}

/// 设置新模式测试数据
pub fn setup_new_wm(num_monitors: usize, num_clients: usize) -> NewWM {
    let mut wm = NewWM::new();
    let mut rng = rand::thread_rng();

    // 添加监视器
    for i in 0..num_monitors {
        wm.add_monitor(i as i32);
    }

    // 添加客户端
    for i in 0..num_clients {
        let monitor_num = rng.gen_range(0..num_monitors) as i32;
        wm.add_client(i as u32, monitor_num);
    }

    wm
}

// ===================================================================
//  基准测试配置
// ===================================================================

const CONFIGS: &[(usize, usize)] = &[
    (5, 100),   // 小规模
    // (10, 1000), // 中规模
    // (20, 5000), // 大规模
];

fn benchmark_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("Client Traversal");

    for &(num_monitors, num_clients) in CONFIGS {
        let config_name = format!("{}mon_{}cli", num_monitors, num_clients);

        // 旧模式
        let old_wm = setup_old_wm(num_monitors, num_clients);
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &old_wm,
            |b, wm| b.iter(|| wm.traverse_all_clients()),
        );

        // 新模式
        let new_wm = setup_new_wm(num_monitors, num_clients);
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &new_wm,
            |b, wm| b.iter(|| wm.traverse_all_clients()),
        );
    }

    group.finish();
}

fn benchmark_add_clients(c: &mut Criterion) {
    let mut group = c.benchmark_group("Add Clients");

    for &(num_monitors, _) in CONFIGS {
        let config_name = format!("{}monitors", num_monitors);

        // 旧模式 - 添加客户端
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &num_monitors,
            |b, &num_mon| {
                b.iter_with_setup(
                    || {
                        let mut wm = OldWM::new();
                        for i in 0..num_mon {
                            wm.add_monitor(i as i32);
                        }
                        wm
                    },
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        for i in 0..100 {
                            let monitor_num = rng.gen_range(0..num_mon) as i32;
                            wm.add_client(i as u32, monitor_num);
                        }
                    },
                )
            },
        );

        // 新模式 - 添加客户端
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &num_monitors,
            |b, &num_mon| {
                b.iter_with_setup(
                    || {
                        let mut wm = NewWM::new();
                        for i in 0..num_mon {
                            wm.add_monitor(i as i32);
                        }
                        wm
                    },
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        for i in 0..100 {
                            let monitor_num = rng.gen_range(0..num_mon) as i32;
                            wm.add_client(i as u32, monitor_num);
                        }
                    },
                )
            },
        );
    }

    group.finish();
}

fn benchmark_find_clients(c: &mut Criterion) {
    let mut group = c.benchmark_group("Find Clients");

    for &(num_monitors, num_clients) in CONFIGS {
        let config_name = format!("{}mon_{}cli", num_monitors, num_clients);

        // 旧模式 - 查找客户端
        let old_wm = setup_old_wm(num_monitors, num_clients);
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &old_wm,
            |b, wm| {
                let mut rng = rand::thread_rng();
                b.iter(|| {
                    let win = rng.gen_range(0..num_clients) as u32;
                    wm.find_client(win)
                })
            },
        );

        // 新模式 - 查找客户端
        let new_wm = setup_new_wm(num_monitors, num_clients);
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &new_wm,
            |b, wm| {
                let mut rng = rand::thread_rng();
                b.iter(|| {
                    let win = rng.gen_range(0..num_clients) as u32;
                    wm.find_client(win)
                })
            },
        );
    }

    group.finish();
}

fn benchmark_remove_clients(c: &mut Criterion) {
    let mut group = c.benchmark_group("Remove Clients");

    for &(num_monitors, num_clients) in CONFIGS {
        let config_name = format!("{}mon_{}cli", num_monitors, num_clients);

        // 旧模式 - 删除客户端
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_old_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        for _ in 0..10 {
                            let win = rng.gen_range(0..num_cli) as u32;
                            wm.remove_client(win);
                        }
                    },
                )
            },
        );

        // 新模式 - 删除客户端
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_new_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        for _ in 0..10 {
                            let win = rng.gen_range(0..num_cli) as u32;
                            wm.remove_client(win);
                        }
                    },
                )
            },
        );
    }

    group.finish();
}

fn benchmark_update_clients(c: &mut Criterion) {
    let mut group = c.benchmark_group("Update Clients");

    for &(num_monitors, num_clients) in CONFIGS {
        let config_name = format!("{}mon_{}cli", num_monitors, num_clients);

        // 旧模式 - 更新客户端
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_old_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        let win = rng.gen_range(0..num_cli) as u32;
                        let geom = ClientGeometry {
                            x: rng.gen_range(0..1920),
                            y: rng.gen_range(0..1080),
                            w: rng.gen_range(100..800),
                            h: rng.gen_range(100..600),
                        };
                        wm.update_client_geometry(win, geom);
                    },
                )
            },
        );

        // 新模式 - 更新客户端
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_new_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        let win = rng.gen_range(0..num_cli) as u32;
                        let geom = ClientGeometry {
                            x: rng.gen_range(0..1920),
                            y: rng.gen_range(0..1080),
                            w: rng.gen_range(100..800),
                            h: rng.gen_range(100..600),
                        };
                        wm.update_client_geometry(win, geom);
                    },
                )
            },
        );
    }

    group.finish();
}

fn benchmark_swap_clients(c: &mut Criterion) {
    let mut group = c.benchmark_group("Swap Adjacent Clients");

    for &(num_monitors, num_clients) in &CONFIGS[..CONFIGS.len()] {
        // 只测试前两个配置
        let config_name = format!("{}mon_{}cli", num_monitors, num_clients);

        // 旧模式 - 交换客户端
        group.bench_with_input(
            BenchmarkId::new("Old_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_old_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        let win1 = rng.gen_range(0..num_cli) as u32;
                        let win2 = rng.gen_range(0..num_cli) as u32;
                        wm.swap_adjacent_clients(win1, win2);
                    },
                )
            },
        );

        // 新模式 - 交换客户端
        group.bench_with_input(
            BenchmarkId::new("New_WM", &config_name),
            &(num_monitors, num_clients),
            |b, &(num_mon, num_cli)| {
                b.iter_with_setup(
                    || setup_new_wm(num_mon, num_cli),
                    |mut wm| {
                        let mut rng = rand::thread_rng();
                        let win1 = rng.gen_range(0..num_cli) as u32;
                        let win2 = rng.gen_range(0..num_cli) as u32;
                        wm.swap_adjacent_clients_safe(win1, win2);
                    },
                )
            },
        );
    }

    group.finish();
}

// 注册所有基准测试
criterion_group!(
    benches,
    benchmark_traversal,
    benchmark_add_clients,
    benchmark_find_clients,
    benchmark_remove_clients,
    benchmark_update_clients,
    benchmark_swap_clients
);
criterion_main!(benches);
