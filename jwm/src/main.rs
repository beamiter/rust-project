// use bar::StatusBar;
use chrono::prelude::*;
use coredump::register_panic_handler;
use flexi_logger::{Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming};
use jwm::Jwm;
use log::{info, warn};
use std::env;

pub fn setup_locale() {
    // 获取当前locale
    let locale = env::var("LANG")
        .or_else(|_| env::var("LC_ALL"))
        .or_else(|_| env::var("LC_CTYPE"))
        .unwrap_or_else(|_| "C".to_string());
    info!("Using locale: {}", locale);
    // 检查UTF-8支持
    if !locale.contains("UTF-8") && !locale.contains("utf8") {
        warn!(
            "Non-UTF-8 locale detected ({}). Text display may be affected.",
            locale
        );
        warn!("Consider setting: export LANG=en_US.UTF-8");
    }
    // 确保关键的locale环境变量存在
    if env::var("LC_CTYPE").is_err() {
        if locale.contains("UTF-8") {
            env::set_var("LC_CTYPE", &locale);
        } else {
            env::set_var("LC_CTYPE", "en_US.UTF-8");
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = register_panic_handler();

    setup_locale();
    jwm::miscellaneous::init_auto_command();
    jwm::miscellaneous::init_auto_start();

    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H_%M_%S").to_string();
    let log_filename = format!("jwm_{}", timestamp);
    Logger::try_with_str("info")
        .unwrap()
        .format(flexi_logger::colored_opt_format)
        .log_to_file(
            FileSpec::default()
                .directory("/tmp/jwm")
                .basename(format!("{log_filename}"))
                .suffix("log"),
        )
        .duplicate_to_stdout(Duplicate::Info)
        // .log_to_stdout()
        // .buffer_capacity(1024)
        // .use_background_worker(true)
        .rotate(
            Criterion::Size(10_000_000),
            Naming::Numbers,
            Cleanup::KeepLogFiles(5),
        )
        .start()
        .unwrap();

    info!("[main] main begin");
    let mut jwm = Jwm::new();
    info!("[main] checkotherwm");
    jwm.checkotherwm()?;
    info!("[main] setup");
    jwm.setup()?;
    info!("[main] scan");
    let _ = jwm.scan();
    info!("[main] run");
    jwm.run_async().await?;
    info!("[main] cleanup");
    jwm.cleanup()?;
    info!("[main] close display");
    info!("[main] end");

    Ok(())
}
