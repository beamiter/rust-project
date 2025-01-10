use battery::Manager;
use std::fs;
use sysinfo::System;

use crate::icon_gallery::generate_random_tags;

pub const BLACK: &str = "#222526";
pub const GREEN: &str = "#89b482";
pub const WHITE: &str = "#c7b89d";
pub const GREY: &str = "#2b2e2f";
pub const BLUE: &str = "#6f8faf";
#[allow(dead_code)]
pub const RED: &str = "#ec6b64";
pub const DARKBLUE: &str = "#6080a0";

pub struct StatusBar {
    icon_list: Vec<&'static str>,
    sys: System,
}
impl StatusBar {
    // Function to read file contents
    fn read_file(&self, path: &str) -> Result<String, std::io::Error> {
        fs::read_to_string(path)
    }

    pub fn new() -> Self {
        StatusBar {
            icon_list: generate_random_tags(20),
            sys: System::new_all(),
        }
    }

    pub fn update_icon_list(&mut self) {
        self.icon_list = generate_random_tags(20);
    }

    // Function to get CPU load
    pub fn cpu_load(&mut self) -> String {
        self.sys.refresh_cpu_all();
        // Wait a bit because CPU usage is based on diff.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        // Refresh CPUs again to get actual value.
        self.sys.refresh_cpu_all();
        format!(
            "^c{}^^b{}^ {} CPU ^c{}^^b{}^ {:.2}%",
            BLACK,
            GREEN,
            self.icon_list[0],
            WHITE,
            GREY,
            self.sys.global_cpu_usage()
        )
    }

    // Function to get battery capacity
    pub fn battery_capacity(&self) -> String {
        // Create an instance of the battery manager
        let manager = Manager::new();

        let mut battery_state = "Plugged".to_string();
        // Get the first battery (assuming there is at least one)
        if let Some(battery) = manager.unwrap().batteries().unwrap().next() {
            let battery = battery.unwrap();

            // Calculate the battery percentage
            let percentage = battery
                .state_of_charge()
                .get::<battery::units::ratio::percent>();

            battery_state = format!("Battery {:.2}%", percentage);
        }
        format!(
            "^c{}^ {} {}",
            BLACK,
            self.icon_list[1],
            battery_state.trim()
        )
    }

    // Function to get memory usage
    pub fn mem_usage(&mut self) -> String {
        self.sys.refresh_memory();
        let unavailable = (self.sys.total_memory() - self.sys.available_memory()) as f64 / 1e9;
        let available = self.sys.available_memory() as f64 / 1e9;
        format!(
            "^c{}^^b{}^ {} ^c{}^{:.1}^c{}^ {} {:.1}",
            BLUE, BLACK, self.icon_list[2], BLUE, unavailable, RED, self.icon_list[3], available
        )
    }

    // Function to get WLAN status
    pub fn wlan_status(&self) -> String {
        // Adjust the interface name pattern to match your system's WLAN interface naming convention.
        let wlan_paths = fs::read_dir("/sys/class/net")
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.to_str().unwrap_or_default().contains("wlan"));

        let mut status = "Disconnected".to_owned();
        for wlan_path in wlan_paths {
            let operstate_path = wlan_path.join("operstate");
            if let Ok(state) = self.read_file(operstate_path.to_str().unwrap()) {
                if state.trim() == "up" {
                    status = "Connected".to_owned();
                    return format!(
                        "^c{}^^b{}^ {} ^d^^c{}^ {}",
                        BLACK, BLUE, self.icon_list[4], BLUE, status
                    );
                }
            }
        }

        format!(
            "^c{}^^b{}^ {} ^d^^c{}^ {}",
            BLACK, BLUE, self.icon_list[5], BLUE, status
        )
    }

    // Function to get current time
    pub fn current_time(&self) -> String {
        let now = chrono::Local::now();
        format!(
            "^c{}^^b{}^ {} ^c{}^^b{}^ {} {}",
            BLACK,
            DARKBLUE,
            self.icon_list[6],
            BLACK,
            BLUE,
            now.format("%d/%m/%Y %H:%M"),
            self.icon_list[7],
        )
    }

    pub fn broadcast_string(&mut self) -> String {
        let status = format!(
            "{} {} {} {} {}",
            self.battery_capacity(),
            self.cpu_load(),
            self.mem_usage(),
            self.wlan_status(),
            self.current_time(),
        );
        return status;
    }
}
