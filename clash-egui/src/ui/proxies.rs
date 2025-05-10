// src/ui/proxies.rs
use crate::clash::api::ProxyInfo;
use crate::clash::core::ClashCore;
use eframe::egui;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Proxies {
    core: Arc<Mutex<ClashCore>>,
    // 代理信息缓存
    proxies: Vec<ProxyInfo>,
    proxy_groups: Vec<ProxyInfo>,
    // 延迟测试结果
    delay_results: HashMap<String, u64>,
    // UI 状态
    selected_group: String,
    search_query: String,
    show_filter_options: bool,
    filter_type: FilterType,
    sort_by: SortBy,
    sort_order: SortOrder,
    // 刷新状态
    last_refresh: Instant,
    is_testing_delay: bool,
    test_progress: f32,
    last_test_time: Option<Instant>,
    // 错误信息
    error_message: Option<String>,
    // 分页
    page_size: usize,
    current_page: usize,
    active_proxy: Option<String>,               // 当前活跃代理
    auto_select_after_test: bool,               // 测速后是否自动选择最快的代理
    show_advanced_options: bool,                // 是否显示高级选项
    selected_proxy_for_details: Option<String>, // 选中查看详情的代理
}

#[derive(Debug, Clone, PartialEq)]
enum FilterType {
    All,
    Shadowsocks,
    Vmess,
    Trojan,
    Socks5,
    Http,
}

#[derive(Debug, Clone, PartialEq)]
enum SortBy {
    Name,
    Type,
    Latency,
}

#[derive(Debug, Clone, PartialEq)]
enum SortOrder {
    Ascending,
    Descending,
}

impl Proxies {
    pub fn new(core: Arc<Mutex<ClashCore>>) -> Self {
        Self {
            core,
            proxies: Vec::new(),
            proxy_groups: Vec::new(),
            delay_results: HashMap::new(),
            selected_group: "".to_string(),
            search_query: "".to_string(),
            show_filter_options: false,
            filter_type: FilterType::All,
            sort_by: SortBy::Name,
            sort_order: SortOrder::Ascending,
            last_refresh: Instant::now() - Duration::from_secs(60), // 强制首次刷新
            is_testing_delay: false,
            test_progress: 0.0,
            last_test_time: None,
            error_message: None,
            page_size: 20,
            current_page: 0,
            active_proxy: None,
            auto_select_after_test: false,
            show_advanced_options: false,
            selected_proxy_for_details: None,
        }
    }
    // 添加显示代理详情的方法
    fn show_proxy_details(&mut self, ui: &mut egui::Ui, proxy_name: &str) {
        // 查找代理信息
        let proxy_info = self.proxies.iter().find(|p| p.name == proxy_name).cloned();

        if let Some(proxy) = proxy_info {
            ui.heading(format!("代理详情: {}", proxy.name));

            ui.horizontal(|ui| {
                ui.label("类型:");
                ui.strong(&proxy.proxy_type);
            });

            // 显示延迟历史
            if !proxy.history.is_empty() {
                ui.label("延迟历史:");

                // 绘制延迟历史图表
                let plot = egui_plot::Plot::new("delay_history")
                    .height(100.0)
                    .show_x(false)
                    .show_y(true)
                    .include_y(0.0)
                    .view_aspect(3.0);

                plot.show(ui, |plot_ui| {
                    let points: Vec<[f64; 2]> = proxy
                        .history
                        .iter()
                        .enumerate()
                        .map(|(i, h)| [i as f64, h.delay as f64])
                        .collect();

                    let line =
                        egui_plot::Line::new("delay-history", egui_plot::PlotPoints::new(points))
                            .color(egui::Color32::GREEN)
                            .width(2.0);

                    plot_ui.line(line);
                });
            }

            // 显示其他代理属性
            ui.horizontal(|ui| {
                ui.label("支持 UDP:");
                ui.label(if proxy.udp.unwrap_or(false) {
                    "是"
                } else {
                    "否"
                });
            });

            // 添加操作按钮
            ui.horizontal(|ui| {
                if ui.button("设为活跃代理").clicked() {
                    self.set_active_proxy(&proxy.name);
                }

                if ui.button("测试延迟").clicked() {
                    self.test_single_proxy(&proxy.name);
                }

                if ui.button("关闭").clicked() {
                    self.selected_proxy_for_details = None;
                }
            });
        } else {
            ui.label("未找到代理信息");

            if ui.button("关闭").clicked() {
                self.selected_proxy_for_details = None;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 如果数据过期，刷新代理列表
        if self.last_refresh.elapsed() > Duration::from_secs(30) {
            self.refresh_proxies();
        }

        ui.horizontal(|ui| {
            ui.heading("    ");
            ui.add_space(8.0);
            if ui.button("🔄 refresh").clicked() {
                self.refresh_proxies();
            }

            ui.add_space(16.0);

            // 如果有错误信息，显示错误
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, error);
                if ui.button("clear").clicked() {
                    self.error_message = None;
                }
            }
        });

        ui.add_space(8.0);

        // 搜索和过滤选项
        ui.horizontal(|ui| {
            ui.label("search:");
            let response = ui.text_edit_singleline(&mut self.search_query);
            if response.changed() {
                self.current_page = 0; // 搜索时重置页码
            }

            ui.add_space(8.0);

            if ui
                .button(if self.show_filter_options {
                    "hiden filter option"
                } else {
                    "show filter option"
                })
                .clicked()
            {
                self.show_filter_options = !self.show_filter_options;
            }
        });

        // 过滤和排序选项
        if self.show_filter_options {
            ui.horizontal(|ui| {
                ui.label("type:");
                ui.radio_value(&mut self.filter_type, FilterType::All, "all");
                ui.radio_value(
                    &mut self.filter_type,
                    FilterType::Shadowsocks,
                    "Shadowsocks",
                );
                ui.radio_value(&mut self.filter_type, FilterType::Vmess, "Vmess");
                ui.radio_value(&mut self.filter_type, FilterType::Trojan, "Trojan");
                ui.radio_value(&mut self.filter_type, FilterType::Socks5, "Socks5");
                ui.radio_value(&mut self.filter_type, FilterType::Http, "Http");
            });

            ui.horizontal(|ui| {
                ui.label("sort:");
                ui.radio_value(&mut self.sort_by, SortBy::Name, "name");
                ui.radio_value(&mut self.sort_by, SortBy::Type, "type");
                ui.radio_value(&mut self.sort_by, SortBy::Latency, "delay");

                ui.add_space(16.0);

                ui.radio_value(&mut self.sort_order, SortOrder::Ascending, "Ascending");
                ui.radio_value(&mut self.sort_order, SortOrder::Descending, "Descending");
            });
        }

        ui.add_space(16.0);

        // 代理组选择
        ui.horizontal(|ui| {
            ui.label("group:");

            egui::ComboBox::from_id_salt("proxy_group_selector")
                .selected_text(if self.selected_group.is_empty() {
                    "choose"
                } else {
                    &self.selected_group
                })
                .show_ui(ui, |ui| {
                    for group in &self.proxy_groups {
                        let is_selected = group.name == self.selected_group;
                        if ui.selectable_label(is_selected, &group.name).clicked() {
                            self.selected_group = group.name.clone();
                            self.current_page = 0; // 切换组时重置页码
                        }
                    }
                });

            ui.add_space(16.0);

            // 延迟测试按钮
            let test_button_text = if self.is_testing_delay {
                format!("testing {}%", (self.test_progress * 100.0) as u32)
            } else {
                "test delay".to_string()
            };

            let test_button =
                ui.add_enabled(!self.is_testing_delay, egui::Button::new(test_button_text));
            if test_button.clicked() {
                self.start_latency_test();
            }

            // 显示上次测试时间
            if let Some(last_test) = self.last_test_time {
                let elapsed = last_test.elapsed();
                let elapsed_str = if elapsed.as_secs() < 60 {
                    format!("{} sec ago", elapsed.as_secs())
                } else if elapsed.as_secs() < 3600 {
                    format!("{} min ago", elapsed.as_secs() / 60)
                } else {
                    format!("{} hour ago", elapsed.as_secs() / 3600)
                };

                ui.add_space(8.0);
                ui.label(format!("last test: {}", elapsed_str));
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_advanced_options, "高级选项");

            if self.show_advanced_options {
                ui.checkbox(&mut self.auto_select_after_test, "测速后自动选择最快代理");

                if ui.button("选择最佳代理").clicked() {
                    self.select_best_proxy();
                }
            }
        });

        // 显示当前活跃代理
        if let Some(active) = self.active_proxy.clone() {
            ui.horizontal(|ui| {
                ui.label("当前活跃代理:");
                ui.strong(active);

                if ui.button("清除选择").clicked() {
                    self.active_proxy = None;
                }
            });
        }

        ui.add_space(16.0);

        // 获取选中组的代理
        let mut proxies_in_group: Vec<String> = Vec::new();
        let mut current_selected: Option<String> = None;

        if !self.selected_group.is_empty() {
            for group in &self.proxy_groups {
                if group.name == self.selected_group {
                    if let Some(all) = &group.all {
                        proxies_in_group = all.clone();
                    }
                    if let Some(now) = &group.now {
                        current_selected = Some(now.clone());
                    }
                    break;
                }
            }
        }

        // 过滤代理
        let filtered_proxies: Vec<ProxyInfo> = self
            .proxies
            .iter()
            .filter(|proxy| {
                // 只显示属于当前选中组的代理
                if !self.selected_group.is_empty() && !proxies_in_group.contains(&proxy.name) {
                    return false;
                }

                // 根据搜索词过滤
                if !self.search_query.is_empty()
                    && !proxy
                        .name
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                {
                    return false;
                }

                // 根据类型过滤
                match self.filter_type {
                    FilterType::All => true,
                    FilterType::Shadowsocks => proxy.proxy_type == "ss",
                    FilterType::Vmess => proxy.proxy_type == "vmess",
                    FilterType::Trojan => proxy.proxy_type == "trojan",
                    FilterType::Socks5 => proxy.proxy_type == "socks5",
                    FilterType::Http => proxy.proxy_type == "http",
                }
            })
            .cloned()
            .collect();

        // 排序代理
        let mut sorted_proxies = filtered_proxies;
        sorted_proxies.sort_by(|a, b| {
            let cmp = match self.sort_by {
                SortBy::Name => a.name.cmp(&b.name),
                SortBy::Type => a.proxy_type.cmp(&b.proxy_type),
                SortBy::Latency => {
                    let delay_a = self.delay_results.get(&a.name).unwrap_or(&u64::MAX);
                    let delay_b = self.delay_results.get(&b.name).unwrap_or(&u64::MAX);
                    delay_a.cmp(delay_b)
                }
            };

            if self.sort_order == SortOrder::Descending {
                cmp.reverse()
            } else {
                cmp
            }
        });

        // 分页
        let total_pages = (sorted_proxies.len() + self.page_size - 1) / self.page_size;
        let start_idx = self.current_page * self.page_size;
        let end_idx = (start_idx + self.page_size).min(sorted_proxies.len());
        let current_page_proxies = &sorted_proxies[start_idx..end_idx];

        // 代理表格
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("proxies_grid")
                .striped(true)
                .spacing([8.0, 8.0])
                .min_col_width(100.0)
                .show(ui, |ui| {
                    // 表头
                    ui.strong("name");
                    ui.strong("type");
                    ui.strong("delay");
                    ui.strong("operation");
                    ui.end_row();

                    // 代理列表
                    for proxy in current_page_proxies {
                        // 名称列
                        let is_current = current_selected
                            .as_ref()
                            .map_or(false, |s| s == &proxy.name);
                        let proxy_name_text = if is_current {
                            egui::RichText::new(&proxy.name)
                                .strong()
                                .color(egui::Color32::GREEN)
                        } else {
                            egui::RichText::new(&proxy.name)
                        };
                        ui.label(proxy_name_text);

                        // 类型列
                        let type_text = match proxy.proxy_type.as_str() {
                            "ss" => "Shadowsocks",
                            "ssr" => "ShadowsocksR",
                            "vmess" => "Vmess",
                            "trojan" => "Trojan",
                            "socks5" => "Socks5",
                            "http" => "Http",
                            "direct" => "Direct",
                            "reject" => "Reject",
                            _ => &proxy.proxy_type,
                        };
                        ui.label(type_text);

                        // 延迟列
                        let delay = self.delay_results.get(&proxy.name).copied();
                        let delay_text = match delay {
                            Some(d) if d < 10000 => format!("{} ms", d),
                            Some(_) => "overtime".to_string(),
                            None => "-".to_string(),
                        };

                        let delay_color = match delay {
                            Some(d) if d < 100 => egui::Color32::GREEN,
                            Some(d) if d < 200 => egui::Color32::from_rgb(144, 238, 144), // 浅绿色
                            Some(d) if d < 300 => egui::Color32::YELLOW,
                            Some(d) if d < 500 => egui::Color32::from_rgb(255, 165, 0), // 橙色
                            Some(_) => egui::Color32::RED,
                            None => egui::Color32::GRAY,
                        };

                        ui.colored_label(delay_color, delay_text);

                        // 操作列
                        ui.horizontal(|ui| {
                            if ui.button("test").clicked() {
                                let proxy_name = proxy.name.clone();
                                self.test_single_proxy(&proxy_name);
                            }
                            // 添加"设为活跃"按钮
                            let is_active = self
                                .active_proxy
                                .as_ref()
                                .map_or(false, |p| p == &proxy.name);
                            if !is_active {
                                if ui.button("设为活跃").clicked() {
                                    self.set_active_proxy(&proxy.name);
                                }
                            } else {
                                ui.label(egui::RichText::new("✓ 活跃").color(egui::Color32::GREEN));
                            }
                            if ui.button("详情").clicked() {
                                self.selected_proxy_for_details = Some(proxy.name.clone());
                            }

                            if !self.selected_group.is_empty() && !is_current {
                                if ui.button("添加到组").clicked() {
                                    let group = self.selected_group.clone();
                                    let proxy_name = proxy.name.clone();
                                    self.switch_proxy(&group, &proxy_name);
                                }
                            }
                        });
                        // 显示代理详情窗口
                        if let Some(proxy_name) = self.selected_proxy_for_details.clone() {
                            egui::Window::new(format!("代理详情: {}", proxy_name))
                                .resizable(true)
                                .collapsible(false)
                                .min_width(400.0)
                                .show(ctx, |ui| {
                                    self.show_proxy_details(ui, &proxy_name);
                                });
                        }

                        ui.end_row();
                    }
                });

            // 分页控件
            if total_pages > 1 {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(self.current_page > 0, |ui| {
                        if ui.button("last page").clicked() {
                            self.current_page -= 1;
                        }
                    });

                    ui.label(format!(
                        "the {} page, total {} pages",
                        self.current_page + 1,
                        total_pages
                    ));

                    ui.add_enabled_ui(self.current_page < total_pages - 1, |ui| {
                        if ui.button("next page").clicked() {
                            self.current_page += 1;
                        }
                    });

                    ui.add_space(16.0);

                    // 跳转到指定页
                    let mut page_input = (self.current_page + 1).to_string();
                    ui.label("switch to:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut page_input)
                            .desired_width(50.0)
                            .hint_text("page num"),
                    );

                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Ok(page) = page_input.parse::<usize>() {
                            if page >= 1 && page <= total_pages {
                                self.current_page = page - 1;
                            }
                        }
                    }

                    if ui.button("switch").clicked() {
                        if let Ok(page) = page_input.parse::<usize>() {
                            if page >= 1 && page <= total_pages {
                                self.current_page = page - 1;
                            }
                        }
                    }
                });
            }
        });

        // 如果正在测试延迟，请求重绘
        if self.is_testing_delay {
            ctx.request_repaint();
        }
    }

    // 添加设置活跃代理的方法
    fn set_active_proxy(&mut self, proxy_name: &str) {
        // 先保存选中的代理名称，避免界面卡顿
        self.active_proxy = Some(proxy_name.to_string());

        // 克隆需要的数据
        let core = self.core.clone();
        let proxy_name = proxy_name.to_string();

        // 使用后台线程执行 API 调用
        std::thread::spawn(move || {
            // 设置超时，避免无限等待
            let result = std::panic::catch_unwind(|| {
                if let Ok(core_guard) = core.lock() {
                    if let Ok(api_client) = core_guard.get_api_client().lock() {
                        return api_client.set_global_proxy(&proxy_name);
                    }
                }
                Err(anyhow::anyhow!("无法获取API客户端"))
            });

            // 处理结果和错误
            let api_result = match result {
                Ok(res) => res,
                Err(_) => Err(anyhow::anyhow!("API调用过程中发生panic")),
            };

            // 将结果发送回主线程
            if let Ok(core_guard) = core.lock() {
                if let Ok(api_client) = core_guard.get_api_client().lock() {
                    if let Some(mut proxies_ui) = api_client.get_app_state_mut() {
                        if let Err(e) = api_result {
                            proxies_ui.set_error(format!("设置全局代理失败: {}", e));
                        } else {
                            // 成功设置后刷新代理列表
                            proxies_ui.schedule_refresh();
                        }
                    }
                }
            }
        });
    }

    // 添加设置错误信息的方法
    fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
    }

    // 添加调度刷新的方法
    fn schedule_refresh(&mut self) {
        self.last_refresh = Instant::now() - Duration::from_secs(60); // 强制下次更新时刷新
    }

    // 添加选择最佳代理的方法
    fn select_best_proxy(&mut self) {
        // 找到延迟最低的代理
        let mut best_proxy = None;
        let mut lowest_delay = u64::MAX;

        for (name, &delay) in &self.delay_results {
            // 跳过延迟太高的代理
            if delay >= 10000 {
                continue;
            }

            // 检查这个代理是否是有效的代理（不是代理组）
            let is_valid_proxy = self.proxies.iter().any(|p| &p.name == name);

            if is_valid_proxy && delay < lowest_delay {
                lowest_delay = delay;
                best_proxy = Some(name.clone());
            }
        }

        // 如果找到了最佳代理，设置为活跃代理
        if let Some(proxy) = best_proxy {
            self.set_active_proxy(&proxy);
        } else {
            self.error_message = Some("没有找到合适的代理".to_string());
        }
    }

    fn refresh_proxies(&mut self) {
        if let Ok(core) = self.core.lock() {
            if let Ok(api_client) = core.get_api_client().lock() {
                match api_client.get_proxies() {
                    Ok(proxies) => {
                        // 分离代理和代理组
                        let mut normal_proxies = Vec::new();
                        let mut proxy_groups = Vec::new();

                        for proxy in proxies {
                            if proxy.all.is_some() {
                                proxy_groups.push(proxy);
                            } else {
                                normal_proxies.push(proxy);
                            }
                        }

                        self.proxies = normal_proxies;
                        self.proxy_groups = proxy_groups;
                        self.last_refresh = Instant::now();
                        self.error_message = None;
                    }
                    Err(e) => {
                        self.error_message = Some(format!("get proxies error: {}", e));
                    }
                }
            }
        }
    }

    fn test_single_proxy(&mut self, proxy_name: &str) {
        if let Ok(core) = self.core.lock() {
            if let Ok(api_client) = core.get_api_client().lock() {
                match api_client.get_proxy_delay(proxy_name) {
                    Ok(delay) => {
                        self.delay_results.insert(proxy_name.to_string(), delay);
                        self.last_test_time = Some(Instant::now());
                        self.error_message = None;
                    }
                    Err(e) => {
                        self.error_message =
                            Some(format!("test proxies {} fail: {}", proxy_name, e));
                    }
                }
            }
        }
    }

    fn start_latency_test(&mut self) {
        // 创建一个线程来执行延迟测试
        let core = self.core.clone();
        let proxies_to_test: Vec<String> = self.proxies.iter().map(|p| p.name.clone()).collect();

        if proxies_to_test.is_empty() {
            self.error_message = Some("no proxies available".to_string());
            return;
        }

        self.is_testing_delay = true;
        self.test_progress = 0.0;

        // 使用标准线程而不是tokio，避免异步运行时问题
        std::thread::spawn(move || {
            let mut results = HashMap::new();
            let total = proxies_to_test.len();

            for (i, proxy_name) in proxies_to_test.iter().enumerate() {
                if let Ok(core_guard) = core.lock() {
                    if let Ok(api_client) = core_guard.get_api_client().lock() {
                        match api_client.get_proxy_delay(proxy_name) {
                            Ok(delay) => {
                                results.insert(proxy_name.clone(), delay);
                            }
                            Err(_) => {
                                results.insert(proxy_name.clone(), 10000); // 超时或错误
                            }
                        }
                    }
                }

                // 更新进度
                let progress = (i + 1) as f32 / total as f32;

                // 将结果发送回主线程
                // 实际应用中应使用通道或其他线程安全的方法
                // 这里简化处理，直接修改共享状态
                if let Ok(this) = core.lock() {
                    if let Ok(api_client) = this.get_api_client().lock() {
                        if let Some(mut proxies_ui) = api_client.get_app_state_mut() {
                            proxies_ui.update_test_progress(progress, results.clone());
                        }
                    }
                }

                // 短暂暂停，避免API请求过于频繁
                std::thread::sleep(Duration::from_millis(100));
            }

            // 完成测试
            if let Ok(this) = core.lock() {
                if let Ok(api_client) = this.get_api_client().lock() {
                    if let Some(mut proxies_ui) = api_client.get_app_state_mut() {
                        proxies_ui.finish_test(results);
                    }
                }
            }
        });
    }

    // 这些方法需要被API客户端调用来更新UI状态
    pub fn update_test_progress(&mut self, progress: f32, partial_results: HashMap<String, u64>) {
        self.test_progress = progress;
        // 合并部分结果
        for (name, delay) in partial_results {
            self.delay_results.insert(name, delay);
        }
    }

    // 修改完成测试的方法，添加自动选择最快代理的功能
    pub fn finish_test(&mut self, results: HashMap<String, u64>) {
        self.delay_results = results;
        self.is_testing_delay = false;
        self.test_progress = 1.0;
        self.last_test_time = Some(Instant::now());

        // 如果启用了自动选择，选择最佳代理
        if self.auto_select_after_test {
            self.select_best_proxy();
        }
    }

    fn switch_proxy(&mut self, group: &str, proxy: &str) {
        let core = self.core.clone();
        if let Ok(core) = core.lock() {
            if let Ok(api_client) = core.get_api_client().lock() {
                match api_client.switch_proxy(group, proxy) {
                    Ok(_) => {
                        // 切换成功后刷新代理列表
                        self.refresh_proxies();
                    }
                    Err(e) => {
                        self.error_message = Some(format!("switch proxy fail: {}", e));
                    }
                }
            }
        }
    }
}
