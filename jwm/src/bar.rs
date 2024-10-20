use std::fs;
use std::process::Command;

// Function to read file contents
fn read_file(path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

// Function to get CPU load
pub fn cpu_load(black: &str, green: &str, white: &str, grey: &str) -> String {
    if let Ok(contents) = read_file("/proc/loadavg") {
        let load_avg: Vec<&str> = contents.split_whitespace().collect();
        format!(
            "^c{}^ ^b{}^ CPU ^c{}^ ^b{}^ {}",
            black, green, white, grey, load_avg[0]
        )
    } else {
        "".to_owned()
        // "Failed to get CPU load".to_owned()
    }
}

// Function to get package updates
pub fn pkg_updates(green: &str) -> String {
    // Replace with your own update checking command
    let output = Command::new("sh")
        .arg("-c")
        .arg("aptitude search '~U' 2>/dev/null | wc -l")
        .output();

    match output {
        Ok(output) => {
            let updates_count = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .unwrap_or(0);
            if updates_count > 0 {
                format!("^c{}^  {} updates", green, updates_count)
            } else {
                format!("^c{}^  Fully Updated", green)
            }
        }
        Err(_) => {
            "".to_owned()
            // "Failed to get package updates".to_owned(),
        }
    }
}

// Function to get battery capacity
pub fn battery_capacity(blue: &str) -> String {
    if let Ok(capacity) = read_file("/sys/class/power_supply/hidpp_battery_0/capacity") {
        format!("^c{}^   {}", blue, capacity.trim())
    } else {
        "".to_owned()
        // "Failed to get battery capacity".to_owned()
    }
}

// Function to get brightness
pub fn brightness(red: &str) -> String {
    if let Ok(brightness) = read_file("/sys/class/backlight/*/brightness") {
        format!("^c{}^   {}", red, brightness.trim())
    } else {
        "".to_owned()
        // "Failed to get brightness".to_owned()
    }
}

// Function to get memory usage
pub fn mem_usage(blue: &str, black: &str) -> String {
    if let Ok(mem_info) = read_file("/proc/meminfo") {
        let lines: Vec<&str> = mem_info.lines().collect();
        if lines.len() > 1 {
            let total_mem = lines[0]
                .replace("MemTotal:", "")
                .replace("kB", "")
                .trim()
                .parse::<i64>()
                .unwrap_or(0);
            let free_mem = lines[1]
                .replace("MemFree:", "")
                .replace("kB", "")
                .trim()
                .parse::<i64>()
                .unwrap_or(0);
            let used_mem = total_mem - free_mem;
            format!("^c{}^ ^b{}^   ^c{}^ {}kB", blue, black, blue, used_mem)
        } else {
            "Failed to get memory usage".to_owned()
        }
    } else {
        "Failed to get memory info".to_owned()
    }
}

// Function to get WLAN status
pub fn wlan_status(black: &str, blue: &str) -> String {
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
                break;
            }
        }
    }

    format!(
        "^c{}^ ^b{}^ 󰤨 ^d^%s ^c{}^{}{}",
        black, blue, "", blue, status
    )
}

// Function to get current time
pub fn current_time(black: &str, darkblue: &str, blue: &str) -> String {
    let now = chrono::Local::now();
    format!(
        "^c{}^ ^b{}^ 󱑆  ^c{}^ ^b{}^ {}",
        black,
        darkblue,
        black,
        blue,
        now.format("%H:%M")
    )
}
