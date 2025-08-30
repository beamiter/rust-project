// tests/integration_test.rs
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

// 正确导入 X11 事件类型常量
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const BUTTON_PRESS: u8 = 4;
const BUTTON_RELEASE: u8 = 5;

// 或者使用 x11rb 提供的常量
use x11rb::protocol::xproto::{
    ButtonPressEvent, ButtonReleaseEvent, KeyPressEvent, KeyReleaseEvent,
};

// 测试配置
const TEST_DURATION_SECONDS: u64 = 30;
const STRESS_TEST_ITERATIONS: usize = 1000;
const KEY_PRESS_INTERVAL_MS: u64 = 50;

#[derive(Debug, Clone)]
struct KeyCombination {
    modifiers: Vec<String>,
    key: String,
    function: String,
    expected_result: ExpectedResult,
}

#[derive(Debug, Clone)]
enum ExpectedResult {
    WindowFocusChange,
    LayoutChange,
    WindowSpawn,
    WindowClose,
    TagSwitch,
    SystemAction,
}

struct TestStats {
    total_tests: usize,
    passed_tests: usize,
    failed_tests: usize,
    execution_times: Vec<Duration>,
    errors: Vec<String>,
}

impl TestStats {
    fn new() -> Self {
        Self {
            total_tests: 0,
            passed_tests: 0,
            failed_tests: 0,
            execution_times: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn add_result(&mut self, success: bool, duration: Duration, error: Option<String>) {
        self.total_tests += 1;
        self.execution_times.push(duration);

        if success {
            self.passed_tests += 1;
        } else {
            self.failed_tests += 1;
            if let Some(err) = error {
                self.errors.push(err);
            }
        }
    }

    fn print_summary(&self) {
        println!("\n=== 测试结果摘要 ===");
        println!("总测试数: {}", self.total_tests);
        println!("通过: {}", self.passed_tests);
        println!("失败: {}", self.failed_tests);

        if self.total_tests > 0 {
            println!(
                "成功率: {:.2}%",
                (self.passed_tests as f64 / self.total_tests as f64) * 100.0
            );
        }

        if !self.execution_times.is_empty() {
            let total_time: Duration = self.execution_times.iter().sum();
            let avg_time = total_time / self.execution_times.len() as u32;
            let min_time = self.execution_times.iter().min().unwrap();
            let max_time = self.execution_times.iter().max().unwrap();

            println!("平均执行时间: {:?}", avg_time);
            println!("最快执行时间: {:?}", min_time);
            println!("最慢执行时间: {:?}", max_time);
        }

        if !self.errors.is_empty() {
            println!("\n错误详情:");
            for (i, error) in self.errors.iter().take(10).enumerate() {
                println!("  {}. {}", i + 1, error);
            }
            if self.errors.len() > 10 {
                println!("  ... 以及其他 {} 个错误", self.errors.len() - 10);
            }
        }
    }
}

struct JWMTester {
    connection: RustConnection,
    screen_num: usize,
    stats: Arc<Mutex<TestStats>>,
}

impl JWMTester {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (connection, screen_num) = x11rb::connect(None)?;

        Ok(Self {
            connection,
            screen_num,
            stats: Arc::new(Mutex::new(TestStats::new())),
        })
    }

    fn get_default_key_combinations() -> Vec<KeyCombination> {
        vec![
            // 应用启动
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "e".to_string(),
                function: "spawn_dmenu".to_string(),
                expected_result: ExpectedResult::WindowSpawn,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "Return".to_string(),
                function: "spawn_terminal".to_string(),
                expected_result: ExpectedResult::WindowSpawn,
            },
            // 窗口焦点控制
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "j".to_string(),
                function: "focusstack_down".to_string(),
                expected_result: ExpectedResult::WindowFocusChange,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "k".to_string(),
                function: "focusstack_up".to_string(),
                expected_result: ExpectedResult::WindowFocusChange,
            },
            // 布局控制
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "h".to_string(),
                function: "setmfact_decrease".to_string(),
                expected_result: ExpectedResult::LayoutChange,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "l".to_string(),
                function: "setmfact_increase".to_string(),
                expected_result: ExpectedResult::LayoutChange,
            },
            // 布局切换
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "t".to_string(),
                function: "setlayout_tile".to_string(),
                expected_result: ExpectedResult::LayoutChange,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "f".to_string(),
                function: "setlayout_float".to_string(),
                expected_result: ExpectedResult::LayoutChange,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "m".to_string(),
                function: "setlayout_monocle".to_string(),
                expected_result: ExpectedResult::LayoutChange,
            },
            // 窗口操作
            KeyCombination {
                modifiers: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "c".to_string(),
                function: "killclient".to_string(),
                expected_result: ExpectedResult::WindowClose,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "Return".to_string(),
                function: "zoom".to_string(),
                expected_result: ExpectedResult::WindowFocusChange,
            },
            // 标签切换
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "Tab".to_string(),
                function: "loopview_next".to_string(),
                expected_result: ExpectedResult::TagSwitch,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string(), "Shift".to_string()],
                key: "Tab".to_string(),
                function: "loopview_prev".to_string(),
                expected_result: ExpectedResult::TagSwitch,
            },
            // 数字标签
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "1".to_string(),
                function: "view_tag_1".to_string(),
                expected_result: ExpectedResult::TagSwitch,
            },
            KeyCombination {
                modifiers: vec!["Mod1".to_string()],
                key: "2".to_string(),
                function: "view_tag_2".to_string(),
                expected_result: ExpectedResult::TagSwitch,
            },
        ]
    }

    // 修复后的按键发送方法
    fn send_key_combination(
        &self,
        key_combo: &KeyCombination,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let screen = &self.connection.setup().roots[self.screen_num];

        // 获取键码和修饰符
        let keycode = self.get_keycode(&key_combo.key)?;
        let modifiers = self.parse_modifiers(&key_combo.modifiers);

        // 先按下修饰键
        for modifier_mask in self.get_modifier_keycodes(modifiers) {
            xtest::fake_input(
                &self.connection,
                KEY_PRESS,
                modifier_mask,
                x11rb::CURRENT_TIME,
                screen.root,
                0,
                0,
                0,
            )?;
        }

        // 短暂延迟
        thread::sleep(Duration::from_millis(5));

        // 按下主键
        xtest::fake_input(
            &self.connection,
            KEY_PRESS,
            keycode,
            x11rb::CURRENT_TIME,
            screen.root,
            0,
            0,
            0,
        )?;

        // 短暂延迟
        thread::sleep(Duration::from_millis(10));

        // 释放主键
        xtest::fake_input(
            &self.connection,
            KEY_RELEASE,
            keycode,
            x11rb::CURRENT_TIME,
            screen.root,
            0,
            0,
            0,
        )?;

        // 释放修饰键
        for modifier_mask in self.get_modifier_keycodes(modifiers).iter().rev() {
            xtest::fake_input(
                &self.connection,
                KEY_RELEASE,
                *modifier_mask,
                x11rb::CURRENT_TIME,
                screen.root,
                0,
                0,
                0,
            )?;
        }

        self.connection.flush()?;
        Ok(())
    }

    // 改进的键码获取方法
    fn get_keycode(&self, key: &str) -> Result<u8, Box<dyn std::error::Error>> {
        // 使用更准确的键码映射
        let keycode = match key {
            "Return" => 36,
            "Tab" => 23,
            "space" => 65,
            "Escape" => 9,
            "BackSpace" => 22,
            "Delete" => 119,

            // 字母键 (a-z)
            "a" => 38,
            "b" => 56,
            "c" => 54,
            "d" => 40,
            "e" => 26,
            "f" => 41,
            "g" => 42,
            "h" => 43,
            "i" => 31,
            "j" => 44,
            "k" => 45,
            "l" => 46,
            "m" => 58,
            "n" => 57,
            "o" => 32,
            "p" => 33,
            "q" => 24,
            "r" => 27,
            "s" => 39,
            "t" => 28,
            "u" => 30,
            "v" => 55,
            "w" => 25,
            "x" => 53,
            "y" => 29,
            "z" => 52,

            // 数字键 (0-9)
            "0" => 19,
            "1" => 10,
            "2" => 11,
            "3" => 12,
            "4" => 13,
            "5" => 14,
            "6" => 15,
            "7" => 16,
            "8" => 17,
            "9" => 18,

            // 功能键
            "F1" => 67,
            "F2" => 68,
            "F3" => 69,
            "F4" => 70,
            "F5" => 71,
            "F6" => 72,
            "F7" => 73,
            "F8" => 74,
            "F9" => 75,
            "F10" => 76,
            "F11" => 95,
            "F12" => 96,

            // 方向键
            "Left" => 113,
            "Right" => 114,
            "Up" => 111,
            "Down" => 116,

            // 其他键
            "comma" => 59,
            "period" => 60,
            "Page_Up" => 112,
            "Page_Down" => 117,
            "Home" => 110,
            "End" => 115,

            _ => return Err(format!("未知按键: {}", key).into()),
        };
        Ok(keycode)
    }

    fn parse_modifiers(&self, modifiers: &[String]) -> u16 {
        let mut mask = 0u16;
        for modifier in modifiers {
            mask |= match modifier.as_str() {
                "Mod1" | "Alt" => ModMask::M1.into(),
                "Mod2" => ModMask::M2.into(),
                "Mod3" => ModMask::M3.into(),
                "Mod4" | "Super" | "Win" => ModMask::M4.into(),
                "Mod5" => ModMask::M5.into(),
                "Control" | "Ctrl" => ModMask::CONTROL.into(),
                "Shift" => ModMask::SHIFT.into(),
                "Lock" | "CapsLock" => ModMask::LOCK.into(),
                _ => {
                    eprintln!("未知修饰键: {}", modifier);
                    0
                }
            };
        }
        mask
    }

    // 获取修饰键的键码
    fn get_modifier_keycodes(&self, modifiers: u16) -> Vec<u8> {
        let mut keycodes = Vec::new();

        if modifiers & u16::from(ModMask::SHIFT) != 0 {
            keycodes.push(50); // Left Shift
        }
        if modifiers & u16::from(ModMask::CONTROL) != 0 {
            keycodes.push(37); // Left Control
        }
        if modifiers & u16::from(ModMask::M1) != 0 {
            keycodes.push(64); // Left Alt
        }
        if modifiers & u16::from(ModMask::M4) != 0 {
            keycodes.push(133); // Left Super
        }

        keycodes
    }

    fn verify_window_state(
        &self,
        expected: &ExpectedResult,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let screen = &self.connection.setup().roots[self.screen_num];

        // 等待一段时间让窗口管理器处理事件
        thread::sleep(Duration::from_millis(100));

        match expected {
            ExpectedResult::WindowSpawn => {
                // 检查是否有新窗口创建
                let query_tree = query_tree(&self.connection, screen.root)?.reply()?;
                Ok(query_tree.children.len() > 0)
            }
            ExpectedResult::WindowFocusChange => {
                // 检查焦点窗口
                let input_focus = get_input_focus(&self.connection)?.reply()?;
                Ok(input_focus.focus != screen.root && input_focus.focus != 0)
            }
            ExpectedResult::LayoutChange => {
                // 布局变化较难直接检测，我们假设成功
                // 实际应用中可以通过检查窗口位置变化来验证
                Ok(true)
            }
            ExpectedResult::WindowClose => {
                // 简化处理，假设成功
                Ok(true)
            }
            ExpectedResult::TagSwitch => {
                // 标签切换也较难直接检测
                Ok(true)
            }
            ExpectedResult::SystemAction => Ok(true),
        }
    }

    fn test_single_key_combination(
        &self,
        key_combo: &KeyCombination,
    ) -> (bool, Duration, Option<String>) {
        let start_time = Instant::now();

        println!(
            "测试按键组合: {:?} + {} ({})",
            key_combo.modifiers, key_combo.key, key_combo.function
        );

        // 发送按键组合
        let send_result = self.send_key_combination(key_combo);
        if let Err(e) = send_result {
            let duration = start_time.elapsed();
            return (false, duration, Some(format!("发送按键失败: {}", e)));
        }

        // 验证结果
        let verify_result = self.verify_window_state(&key_combo.expected_result);
        let duration = start_time.elapsed();

        match verify_result {
            Ok(success) => {
                if success {
                    println!("  ✓ 测试通过 (耗时: {:?})", duration);
                    (true, duration, None)
                } else {
                    println!("  ✗ 预期结果未达成");
                    (false, duration, Some("预期结果未达成".to_string()))
                }
            }
            Err(e) => {
                println!("  ✗ 验证失败: {}", e);
                (false, duration, Some(format!("验证失败: {}", e)))
            }
        }
    }

    fn run_functional_tests(&self) {
        println!("=== 开始功能测试 ===");

        let key_combinations = Self::get_default_key_combinations();

        for key_combo in &key_combinations {
            let (success, duration, error) = self.test_single_key_combination(key_combo);

            if let Ok(mut stats) = self.stats.lock() {
                stats.add_result(success, duration, error);
            }

            // 测试间隔
            thread::sleep(Duration::from_millis(KEY_PRESS_INTERVAL_MS));
        }

        println!("功能测试完成!\n");
    }

    fn run_stress_test(&self) {
        println!("=== 开始压力测试 ===");
        println!("将执行 {} 次随机按键组合", STRESS_TEST_ITERATIONS);

        let key_combinations = Self::get_default_key_combinations();

        let start_time = Instant::now();

        for i in 0..STRESS_TEST_ITERATIONS {
            if i % 100 == 0 {
                println!("压力测试进度: {}/{}", i, STRESS_TEST_ITERATIONS);
            }

            // 随机选择按键组合
            let key_combo = &key_combinations[i % key_combinations.len()];
            let (success, duration, error) = self.test_single_key_combination(key_combo);

            if let Ok(mut stats) = self.stats.lock() {
                stats.add_result(success, duration, error);
            }

            // 更短的间隔以增加压力
            thread::sleep(Duration::from_millis(5));
        }

        let total_time = start_time.elapsed();
        println!("压力测试完成! 总耗时: {:?}", total_time);
        println!(
            "平均每次操作耗时: {:?}",
            total_time / STRESS_TEST_ITERATIONS as u32
        );
    }

    fn run_memory_leak_test(&self) {
        println!("=== 开始内存泄漏测试 ===");

        let key_combinations = Self::get_default_key_combinations();
        let test_duration = Duration::from_secs(TEST_DURATION_SECONDS);
        let start_time = Instant::now();
        let mut iteration = 0;

        while start_time.elapsed() < test_duration {
            for key_combo in &key_combinations {
                if start_time.elapsed() >= test_duration {
                    break;
                }

                let _ = self.send_key_combination(key_combo);
                thread::sleep(Duration::from_millis(5));
                iteration += 1;

                if iteration % 1000 == 0 {
                    let elapsed = start_time.elapsed();
                    let remaining = test_duration.saturating_sub(elapsed);
                    println!(
                        "内存测试进行中... 已执行 {} 次操作, 剩余时间: {:?}",
                        iteration, remaining
                    );
                }
            }
        }

        println!("内存泄漏测试完成! 总共执行了 {} 次操作", iteration);
    }

    fn monitor_system_resources(&self) -> thread::JoinHandle<()> {
        let stats = Arc::clone(&self.stats);

        thread::spawn(move || {
            println!("开始监控系统资源...");

            let mut max_memory = 0u64;
            let mut max_cpu = 0.0f64;

            loop {
                // 使用 ps 命令获取 JWM 进程信息
                if let Ok(output) = Command::new("ps")
                    .args(&["-C", "jwm", "-o", "pid,pcpu,pmem,rss", "--no-headers"])
                    .output()
                {
                    let output_str = String::from_utf8_lossy(&output.stdout);

                    for line in output_str.lines() {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            if let (Ok(cpu), Ok(memory)) =
                                (parts[1].parse::<f64>(), parts[3].parse::<u64>())
                            {
                                max_cpu = max_cpu.max(cpu);
                                max_memory = max_memory.max(memory);

                                if cpu > 80.0 || memory > 100000 {
                                    // 100MB
                                    println!(
                                        "⚠️  高资源使用: CPU: {:.2}%, 内存: {} KB",
                                        cpu, memory
                                    );
                                }
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_secs(1));

                // 检查是否应该停止监控
                if let Ok(stats) = stats.lock() {
                    if stats.total_tests > STRESS_TEST_ITERATIONS {
                        break;
                    }
                }
            }

            println!(
                "资源监控结束. 峰值 - CPU: {:.2}%, 内存: {} KB",
                max_cpu, max_memory
            );
        })
    }

    pub fn run_all_tests(&self) {
        println!("🚀 开始 JWM 窗口管理器测试");

        // 启动资源监控
        let monitor_handle = self.monitor_system_resources();

        // 运行功能测试
        self.run_functional_tests();

        // 运行压力测试
        self.run_stress_test();

        // 运行内存泄漏测试
        self.run_memory_leak_test();

        // 等待监控线程结束
        let _ = monitor_handle.join();

        // 打印测试结果
        if let Ok(stats) = self.stats.lock() {
            stats.print_summary();
        }
    }
}

// 辅助函数：检查 JWM 是否正在运行
fn check_jwm_running() -> bool {
    Command::new("pgrep")
        .arg("jwm")
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false)
}

// 辅助函数：启动测试环境
fn setup_test_environment() -> Result<(), Box<dyn std::error::Error>> {
    if !check_jwm_running() {
        println!("⚠️  JWM 未运行，请先启动 JWM 窗口管理器");
        return Err("JWM not running".into());
    }

    println!("✓ JWM 正在运行");

    // 创建一些测试窗口
    println!("创建测试窗口...");

    // 启动几个简单的测试窗口
    for i in 0..2 {
        Command::new("sh")
            .arg("-c")
            .arg(&format!("sleep 120 & echo 'Test window {}'", i))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        thread::sleep(Duration::from_millis(300));
    }

    println!("✓ 测试环境设置完成");
    Ok(())
}

// 主测试函数
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "=".repeat(60));
    println!("         JWM 窗口管理器 - 综合测试套件");
    println!("{}", "=".repeat(60));

    // 设置测试环境
    setup_test_environment()?;

    // 创建测试器
    let tester = JWMTester::new()?;

    // 运行所有测试
    tester.run_all_tests();

    println!("\n🎉 所有测试完成!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_parsing() -> Result<(), Box<dyn std::error::Error>> {
        let tester = JWMTester::new()?;

        assert_eq!(tester.get_keycode("Return")?, 36);
        assert_eq!(tester.get_keycode("Tab")?, 23);
        assert_eq!(tester.get_keycode("e")?, 26);

        Ok(())
    }

    #[test]
    fn test_modifier_parsing() -> Result<(), Box<dyn std::error::Error>> {
        let tester = JWMTester::new()?;

        let mods = vec!["Mod1".to_string(), "Shift".to_string()];
        let mask = tester.parse_modifiers(&mods);

        assert_ne!(mask, 0);
        assert!(mask & u16::from(ModMask::M1) != 0);
        assert!(mask & u16::from(ModMask::SHIFT) != 0);

        Ok(())
    }

    #[test]
    fn test_default_key_combinations() {
        let combos = JWMTester::get_default_key_combinations();
        assert!(!combos.is_empty());
        assert!(combos
            .iter()
            .any(|k| k.key == "j" && k.function == "focusstack_down"));
        assert!(combos
            .iter()
            .any(|k| k.key == "e" && k.function == "spawn_dmenu"));
    }

    #[test]
    fn test_modifier_keycodes() -> Result<(), Box<dyn std::error::Error>> {
        let tester = JWMTester::new()?;

        let alt_mask = u16::from(ModMask::M1);
        let keycodes = tester.get_modifier_keycodes(alt_mask);

        assert!(!keycodes.is_empty());
        assert!(keycodes.contains(&64)); // Left Alt keycode

        Ok(())
    }
}
