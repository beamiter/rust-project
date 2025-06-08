//! Integration tests for egui_bar

use egui_bar::{
    audio::AudioManager,
    config::AppConfig,
    system::SystemMonitor,
    utils::{PerformanceMetrics, RollingAverage},
};
use std::time::Duration;

#[test]
fn test_audio_manager_initialization() {
    let audio_manager = AudioManager::new();

    // Should initialize without panicking
    let devices = audio_manager.get_devices();
    println!("Found {} audio devices", devices.len());

    // Should have at least some devices on most systems
    // Note: This might fail in CI environments without audio
}

#[test]
fn test_system_monitor() {
    let mut monitor = SystemMonitor::new(10);

    // Initial refresh
    monitor.refresh();

    // Should have system information
    let snapshot = monitor.get_snapshot();
    assert!(snapshot.is_some());

    if let Some(snapshot) = snapshot {
        assert!(snapshot.memory_total > 0);
        assert!(!snapshot.cpu_usage.is_empty());
        println!("System has {} CPU cores", snapshot.cpu_usage.len());
        println!("Total memory: {:.2} GB", snapshot.memory_total as f64 / 1e9);
    }
}

#[test]
fn test_rolling_average() {
    let mut avg = RollingAverage::new(3);

    assert_eq!(avg.average(), 0.0);
    assert!(avg.is_empty());

    avg.add(1.0);
    assert_eq!(avg.average(), 1.0);
    assert_eq!(avg.len(), 1);

    avg.add(2.0);
    avg.add(3.0);
    assert_eq!(avg.average(), 2.0); // (1+2+3)/3
    assert_eq!(avg.len(), 3);

    // Test rolling behavior
    avg.add(4.0);
    assert_eq!(avg.average(), 3.0); // (2+3+4)/3, 1.0 was dropped
    assert_eq!(avg.len(), 3);
}

#[test]
fn test_performance_metrics() {
    let mut metrics = PerformanceMetrics::new();

    // Start a frame
    metrics.start_frame();

    // Simulate some processing time
    std::thread::sleep(Duration::from_millis(1));

    // Record some metrics
    metrics.record_render_time(Duration::from_millis(5));
    metrics.record_update_time(Duration::from_millis(2));

    // Start another frame to get timing data
    std::thread::sleep(Duration::from_millis(10));
    metrics.start_frame();

    assert!(metrics.frame_count() >= 1);
    println!("Average FPS: {:.2}", metrics.average_fps());
    println!("Frame time: {:.2}ms", metrics.average_frame_time_ms());
}

#[test]
fn test_config_save_load() {
    use std::env;
    use tempfile::tempdir;

    // Create temporary directory
    let temp_dir = tempdir().unwrap();
    let temp_path = temp_dir.path().join("egui_bar").join("config.toml");

    // Set temporary config directory
    env::set_var("XDG_CONFIG_HOME", temp_dir.path());

    // Create and save config
    let mut original_config = AppConfig::default();
    original_config.ui.font_size = 18.0;
    original_config.ui.show_seconds = true;

    assert!(original_config.save().is_ok());

    // Load config back
    let loaded_config = AppConfig::load().unwrap();
    assert_eq!(loaded_config.ui.font_size, 18.0);
    assert_eq!(loaded_config.ui.show_seconds, true);

    // Cleanup
    env::remove_var("XDG_CONFIG_HOME");
}

#[cfg(feature = "audio_tests")]
#[test]
fn test_audio_device_control() {
    let mut audio_manager = AudioManager::new();

    if let Some(master_device) = audio_manager.get_master_device() {
        let device_name = master_device.name.clone();
        let original_volume = master_device.volume;
        let original_mute = master_device.is_muted;

        // Test volume adjustment
        if master_device.has_volume_control {
            let result = audio_manager.adjust_volume(&device_name, 5);
            assert!(result.is_ok());

            // Restore original volume
            let _ = audio_manager.set_volume(&device_name, original_volume, original_mute);
        }

        // Test mute toggle
        if master_device.has_switch_control {
            let result = audio_manager.toggle_mute(&device_name);
            assert!(result.is_ok());

            // Restore original state
            let _ = audio_manager.set_volume(&device_name, original_volume, original_mute);
        }
    }
}

#[test]
fn test_theme_management() {
    use egui_bar::ui::{ThemeManager, ThemeType};

    let mut theme_manager = ThemeManager::new(ThemeType::Dark);

    assert_eq!(*theme_manager.current_theme(), ThemeType::Dark);

    theme_manager.toggle_theme();
    assert_eq!(*theme_manager.current_theme(), ThemeType::Light);

    theme_manager.set_theme(ThemeType::Auto);
    assert_eq!(*theme_manager.current_theme(), ThemeType::Auto);
}

// Benchmark tests (run with `cargo test --release -- --ignored bench`)
#[test]
#[ignore]
fn bench_system_monitor_refresh() {
    let mut monitor = SystemMonitor::new(60);
    let start = std::time::Instant::now();

    for _ in 0..100 {
        monitor.refresh();
    }

    let elapsed = start.elapsed();
    println!("100 system refreshes took: {:?}", elapsed);
    println!("Average per refresh: {:?}", elapsed / 100);
}

#[test]
#[ignore]
fn bench_audio_device_scan() {
    let start = std::time::Instant::now();

    for _ in 0..50 {
        let mut audio_manager = AudioManager::new();
        let _ = audio_manager.refresh_devices();
    }

    let elapsed = start.elapsed();
    println!("50 audio device scans took: {:?}", elapsed);
    println!("Average per scan: {:?}", elapsed / 50);
}
