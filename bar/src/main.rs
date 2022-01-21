use chrono::prelude::*;
use std::thread::sleep;
use std::time::Duration;
use sysinfo::{DiskExt, NetworkExt, ProcessorExt, System, SystemExt};
use termion::{clear, color, cursor, style};

#[allow(dead_code)]
fn draw_colorful_system(sys: &mut System) {
    sys.refresh_all();
    print!(
        "{}{}{}",
        clear::BeforeCursor,
        cursor::Goto(1, 1),
        style::Reset
    );

    println!("{}        Systems", color::Fg(color::Blue));
    println!(
        "Name:             {:?}",
        sys.name().unwrap_or("nan".to_string())
    );
    println!(
        "Kernel version:   {:?}",
        sys.kernel_version().unwrap_or("nan".to_string())
    );
    println!(
        "OS version:       {:?}",
        sys.os_version().unwrap_or("nan".to_string())
    );
    println!(
        "host name:        {:?}",
        sys.host_name().unwrap_or("nan".to_string())
    );
    println!("Processors:       {}", sys.processors().len());

    println!("{}        Disks", color::Fg(color::Red));
    for disk in sys.disks() {
        if let Some("/") = disk.mount_point().to_str() {
            // println!("{:?}", disk);
            println!(
                "available/total: {}/{} GB",
                disk.available_space() as f32 / 1e9,
                disk.total_space() as f32 / 1e9
            );
        }
    }

    println!("{}        Temperatures", color::Fg(color::Magenta));
    for component in sys.components() {
        println!("{:?}", component);
    }

    println!("{}        Networks", color::Fg(color::Cyan));
    for (interface_name, data) in sys.networks() {
        println!(
            "recv: {:05} KB, trans: {:05} KB,     {}",
            data.received() / 1000,
            data.transmitted() / 1000,
            interface_name
        );
    }

    println!("{}        Memories", color::Fg(color::Green));
    println!("total:           {} MB", sys.total_memory() / 1000);
    println!("used:            {} MB", sys.used_memory() / 1000);
    println!("available:       {} MB", sys.available_memory() / 1000);
    println!("free:            {} MB", sys.free_memory() / 1000);

    println!(
        "{}{}{}        Clock",
        color::Fg(color::LightMagenta),
        style::Bold,
        style::Italic
    );
    let now: DateTime<Local> = Local::now();
    let hour = now.hour();
    println!(
        "H::m::s          {:02}/{:02}/{:04} {:02}:{:02}:{:02}",
        now.day(),
        now.month(),
        now.year(),
        hour,
        now.minute(),
        now.second()
    );
}

fn main() {
    println!("Draw colorful system info");
    let mut sys = System::new_all();
    loop {
        sys.refresh_all();
        //draw_colorful_system(&mut sys);
        let now: DateTime<Local> = Local::now();
        let hour = now.hour();
        println!(
            "{:02}/{:02}/{:04} {:02}:{:02}:{:02}",
            now.day(),
            now.month(),
            now.year(),
            hour,
            now.minute(),
            now.second()
        );
        let mut max_cpu_usage: f32 = 0.0;
        for processor in sys.processors() {
            if max_cpu_usage < processor.cpu_usage() {
                max_cpu_usage = processor.cpu_usage();
            }
        }
        println!("{:?}", max_cpu_usage);
        sleep(Duration::new(1, 0));
    }
}
