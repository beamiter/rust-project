use chrono::prelude::*;
use std::thread::sleep;
use std::time::Duration;
use sysinfo::{DiskExt, NetworkExt, System, SystemExt};
use termion::{clear, color, cursor, style};

fn draw_colorful_system(sys: &mut System) {
    sys.refresh_all();
    print!("{}{}{}", clear::BeforeCursor, cursor::Goto(1, 1), style::Reset);

    println!("{}        Disks", color::Fg(color::Red));
    for disk in sys.get_disks() {
        if let Some("/") = disk.get_mount_point().to_str() {
            // println!("{:?}", disk);
            println!(
                "available/total: {}/{} GB",
                disk.get_available_space() as f32 / 1e9,
                disk.get_total_space() as f32 / 1e9
            );
        }
    }

    println!("{}Networks", color::Fg(color::Cyan));
    for (interface_name, data) in sys.get_networks() {
        println!(
            "recv: {:05} KB, trans: {:05} KB,     {}",
            data.get_received() / 1000,
            data.get_transmitted() / 1000,
            interface_name
        );
    }

    println!("{}        Temperatures", color::Fg(color::Magenta));
    for component in sys.get_components() {
        println!("{:?}", component);
    }

    println!("{}        Memory", color::Fg(color::Green));
    println!(
        "total memory:           {} MB",
        sys.get_total_memory() / 1000
    );
    println!(
        "used memory:            {} MB",
        sys.get_used_memory() / 1000
    );
    println!(
        "available memory:       {} MB",
        sys.get_available_memory() / 1000
    );
    println!(
        "free memory:            {} MB",
        sys.get_free_memory() / 1000
    );
    println!("NB processors:          {}", sys.get_processors().len());

    // for (pid, process) in sys.get_processes() {
    // println!("[{}] {} {:?}", pid, process.name(), process.disk_usage());
    // }

    println!("{}        Systems", color::Fg(color::Blue));
    println!(
        "System name:             {:?}",
        sys.get_name().unwrap_or("nan".to_string())
    );
    println!(
        "System kernel version:   {:?}",
        sys.get_kernel_version().unwrap_or("nan".to_string())
    );
    println!(
        "System OS version:       {:?}",
        sys.get_os_version().unwrap_or("nan".to_string())
    );
    println!(
        "System host name:        {:?}",
        sys.get_host_name().unwrap_or("nan".to_string())
    );

    println!(
        "{}{}{}        Clock",
        color::Fg(color::LightMagenta),
        style::Bold,
        style::Italic
    );
    let now: DateTime<Local> = Local::now();
    let hour = now.hour();
    println!(
        "                    {:02}:{:02}:{:02}",
        hour,
        now.minute(),
        now.second()
    );
}

fn main() {
    println!("Draw colorful system info");
    let mut sys = System::new_all();
    loop {
        draw_colorful_system(&mut sys);
        sleep(Duration::new(1, 0));
    }
}
