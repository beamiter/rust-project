use battery::{Manager, State};
use chrono::prelude::*;
use std::thread::sleep;
use std::time::Duration;
use sysinfo::{Components, Disks, Networks, System};
use termion::{clear, color, cursor, style};

fn draw_colorful_system(sys: &mut System) {
    sys.refresh_all();
    print!(
        "{}{}{}",
        clear::BeforeCursor,
        cursor::Goto(1, 1),
        style::Reset
    );
    println!("{}        System", color::Fg(color::Green));
    // RAM and swap information:
    println!("total memory: {} MB", sys.total_memory() as f64 / 1e6);
    println!("used memory : {} MB", sys.used_memory() as f64 / 1e6);
    println!("total swap  : {} MB", sys.total_swap() as f64 / 1e6);
    println!("used swap   : {} MB", sys.used_swap() as f64 / 1e6);
    println!("available   : {} MB", sys.available_memory() as f64 / 1e6);
    println!("free        : {} MB", sys.free_memory() as f64 / 1e6);

    // Display system information:
    println!("System name:             {:?}", System::name());
    println!("System kernel version:   {:?}", System::kernel_version());
    println!("System OS version:       {:?}", System::os_version());
    println!("System host name:        {:?}", System::host_name());

    // Number of CPUs:
    println!("NB CPUs: {}", sys.cpus().len());

    // Display processes ID, name na disk usage:
    // for (pid, process) in sys.processes() {
    //     println!("[{pid}] {:?} {:?}", process.name(), process.disk_usage());
    // }

    // We display all disks' information:
    println!("        Disks");
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        println!("{disk:?}");
    }

    println!("{}        Components", color::Fg(color::Magenta));
    // Components temperature:
    let components = Components::new_with_refreshed_list();
    for component in &components {
        println!("{component:?}");
    }

    println!("{}        Networks", color::Fg(color::Cyan));
    // Network interfaces name, total data received and total data transmitted:
    let networks = Networks::new_with_refreshed_list();
    for (interface_name, data) in &networks {
        println!(
            "{interface_name}: {} B (down) / {} B (up)",
            data.total_received(),
            data.total_transmitted(),
        );
        // If you want the amount of data received/transmitted since last call
        // to `Networks::refresh`, use `received`/`transmitted`.
    }

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

    println!("{}        Others", color::Fg(color::Green));
    let load_avg = System::load_average();
    println!(
        "one minute: {}%, five minutes: {}%, fifteen minutes: {}%",
        load_avg.one, load_avg.five, load_avg.fifteen,
    );

    // Create an instance of the battery manager
    let manager = Manager::new();

    // Get the first battery (assuming there is at least one)
    if let Some(battery) = manager.unwrap().batteries().unwrap().next() {
        let battery = battery.unwrap();

        // Calculate the battery percentage
        let percentage = battery
            .state_of_charge()
            .get::<battery::units::ratio::percent>();

        // Check the state of the battery (Charging, Discharging, Full, etc.)
        println!("{}", battery.state());
        match battery.state() {
            State::Charging => println!("Battery is charging: {:.2}%", percentage),
            State::Discharging => println!("Battery is discharging: {:.2}%", percentage),
            State::Full => println!("Battery is full: {:.2}%", percentage),
            _ => println!("Battery state: {:?}, {:.2}%", battery.state(), percentage),
        }
    } else {
        println!("No battery found.");
    }
}

fn main() {
    println!("Draw colorful system info");
    let mut sys = System::new_all();
    loop {
        draw_colorful_system(&mut sys);
        sleep(Duration::new(1, 0));
    }
}
