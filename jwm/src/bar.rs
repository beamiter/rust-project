use battery::Manager;
use std::fs;
use sysinfo::{CpuRefreshKind, RefreshKind, System};

pub const BLACK: &str = "#222526";
pub const GREEN: &str = "#89b482";
pub const WHITE: &str = "#c7b89d";
pub const GREY: &str = "#2b2e2f";
pub const BLUE: &str = "#6f8faf";
#[allow(dead_code)]
pub const RED: &str = "#ec6b64";
pub const DARKBLUE: &str = "#6080a0";

// Function to read file contents
fn read_file(path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

// Function to get CPU load
pub fn cpu_load() -> String {
    let mut s =
        System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::everything()));
    // Wait a bit because CPU usage is based on diff.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    // Refresh CPUs again to get actual value.
    s.refresh_cpu_usage();
    format!(
        "^c{}^^b{}^ ☘ CPU ^c{}^^b{}^ {:.2}%",
        BLACK,
        GREEN,
        WHITE,
        GREY,
        s.global_cpu_usage()
    )
}

// Function to get battery capacity
pub fn battery_capacity() -> String {
    // Create an instance of the battery manager
    let manager = Manager::new();

    let mut battery_state = "No Battery".to_string();
    // Get the first battery (assuming there is at least one)
    if let Some(battery) = manager.unwrap().batteries().unwrap().next() {
        let battery = battery.unwrap();

        // Calculate the battery percentage
        let percentage = battery
            .state_of_charge()
            .get::<battery::units::ratio::percent>();

        battery_state = format!("{:?} {:.2}%", battery.state(), percentage);
    }
    format!("^c{}^  {}", BLUE, battery_state.trim())
}

// Function to get memory usage
pub fn mem_usage() -> String {
    let mut sys = System::new_all();
    sys.refresh_all();
    let used = sys.used_memory() as f64 / 1e9;
    let free = sys.free_memory() as f64 / 1e9;
    format!(
        "^c{}^^b{}^   ^c{}^{:.1}^c{}^ ◔ {:.1}",
        BLUE, BLACK, BLUE, used, RED, free
    )
}

// Function to get WLAN status
pub fn wlan_status() -> String {
    // Adjust the interface name pattern to match your system's WLAN interface naming convention.
    let wlan_paths = fs::read_dir("/sys/class/net")
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_str().unwrap_or_default().contains("wlan"));

    let mut status = "Disconnected".to_owned();
    for wlan_path in wlan_paths {
        let operstate_path = wlan_path.join("operstate");
        if let Ok(state) = read_file(operstate_path.to_str().unwrap()) {
            if state.trim() == "up" {
                status = "Connected".to_owned();
                return format!("^c{}^^b{}^ 󰤨  ^d^^c{}^ {}", BLACK, BLUE, BLUE, status);
            }
        }
    }

    format!("^c{}^^b{}^ 󰤭  ^d^^c{}^ {}", BLACK, BLUE, BLUE, status)
}

// Function to get current time
pub fn current_time() -> String {
    let now = chrono::Local::now();
    format!(
        "^c{}^^b{}^ 󱑆  ^c{}^^b{}^ {} ⌘ ",
        BLACK,
        DARKBLUE,
        BLACK,
        BLUE,
        now.format("%d/%m/%Y %H:%M")
    )
}
