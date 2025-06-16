//! Volume control window component

use crate::app::events::AppEvent;
use crate::app::state::AppState;
use crate::constants::{colors, icons};
use egui::{Align, Layout};
use egui_twemoji::EmojiLabel;
use log::error;
use std::sync::mpsc;
use std::time::Instant;

/// Volume control window component
#[allow(dead_code)]
pub struct VolumeControlWindow {
    last_render_time: Instant,
}

impl VolumeControlWindow {
    pub fn new() -> Self {
        Self {
            last_render_time: Instant::now(),
        }
    }

    /// Draw volume control window, returns true if window was closed
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        if !app_state.ui_state.volume_window.open {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🔊 音量控制")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .default_pos(
                app_state
                    .ui_state
                    .volume_window
                    .position
                    .unwrap_or_else(|| {
                        let screen_rect = ctx.screen_rect();
                        egui::pos2(
                            screen_rect.center().x - 160.0,
                            screen_rect.center().y - 150.0,
                        )
                    }),
            )
            .open(&mut window_open)
            .show(ctx, |ui| {
                // Save window position
                if let Some(rect) = ctx.memory(|mem| mem.area_rect(ui.id())) {
                    app_state.ui_state.volume_window.position = Some(rect.left_top());
                }

                self.draw_content(ui, app_state, event_sender);

                // Close button
                ui.horizontal(|ui| {
                    if ui.button("🔧 高级混音器").clicked() {
                        let _ = std::process::Command::new("terminator")
                            .args(["-e", "alsamixer"])
                            .spawn();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✖ 关闭").clicked() {
                            app_state.ui_state.toggle_volume_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            app_state.ui_state.toggle_volume_window();
        }
    }

    fn draw_content(
        &mut self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        // 先获取设备信息，避免后续的借用冲突
        let devices: Vec<crate::audio::AudioDevice> =
            app_state.audio_manager.get_devices().to_vec();

        if devices.is_empty() {
            EmojiLabel::new("❌ 没有找到可控制的音频设备").show(ui);
            return;
        }

        // Filter controllable devices - 现在使用 owned 数据
        let controllable_devices: Vec<(usize, crate::audio::AudioDevice)> = devices
            .into_iter()
            .enumerate()
            .filter(|(_, d)| d.has_volume_control || d.has_switch_control)
            .collect();

        if controllable_devices.is_empty() {
            EmojiLabel::new("❌ 没有找到可控制的音频设备").show(ui);
            return;
        }

        // Device selection
        self.draw_device_selector(ui, app_state, &controllable_devices);

        ui.add_space(10.0);

        // Device controls - 现在使用 owned 数据
        if let Some((_, device)) =
            controllable_devices.get(app_state.ui_state.volume_window.selected_device)
        {
            self.draw_device_controls(ui, device, app_state, event_sender);
        }
    }

    fn draw_device_selector(
        &self,
        ui: &mut egui::Ui,
        app_state: &mut AppState,
        controllable_devices: &[(usize, crate::audio::AudioDevice)],
    ) {
        ui.horizontal(|ui| {
            EmojiLabel::new("🎵 设备：").show(ui);

            // Ensure selected device index is valid
            if app_state.ui_state.volume_window.selected_device >= controllable_devices.len() {
                app_state.ui_state.volume_window.selected_device = 0;
            }

            let current_selection =
                &controllable_devices[app_state.ui_state.volume_window.selected_device];

            egui::ComboBox::from_id_salt("audio_device_selector")
                .selected_text(&current_selection.1.description)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for (idx, (_, device)) in controllable_devices.iter().enumerate() {
                        if ui
                            .selectable_label(
                                app_state.ui_state.volume_window.selected_device == idx,
                                &device.description,
                            )
                            .clicked()
                        {
                            app_state.ui_state.volume_window.selected_device = idx;
                        }
                    }
                });
        });
    }

    fn draw_device_controls(
        &mut self,
        ui: &mut egui::Ui,
        device: &crate::audio::AudioDevice,
        app_state: &mut AppState,
        event_sender: &mpsc::Sender<AppEvent>,
    ) {
        let device_name = device.name.clone();
        let mut current_volume = device.volume;
        let is_muted = device.is_muted;

        // Volume control
        if device.has_volume_control {
            ui.horizontal(|ui| {
                EmojiLabel::new("🔊 音量：").show(ui);

                // Mute button
                if device.has_switch_control {
                    let mute_icon = if is_muted {
                        icons::VOLUME_MUTED
                    } else {
                        icons::VOLUME_HIGH
                    };
                    let mute_btn = ui.button(mute_icon);

                    if mute_btn.clicked() {
                        if let Err(e) = event_sender.send(AppEvent::ToggleMute(device_name.clone()))
                        {
                            error!("Failed to send mute toggle event: {}", e);
                        }
                    }

                    mute_btn.on_hover_text(if is_muted { "取消静音" } else { "静音" });
                }

                // Volume percentage
                ui.label(format!("{}%", current_volume));
            });

            // Volume slider
            let slider_response = ui.add(
                egui::Slider::new(&mut current_volume, 0..=100)
                    .show_value(false)
                    .text(""),
            );

            if slider_response.changed()
                && app_state
                    .ui_state
                    .volume_window
                    .should_apply_volume_change()
            {
                if let Err(e) =
                    app_state
                        .audio_manager
                        .set_volume(&device_name, current_volume, is_muted)
                {
                    error!("Failed to set volume: {}", e);
                }
            }
        } else if device.has_switch_control {
            // Switch-only device
            ui.horizontal(|ui| {
                let btn_text = if is_muted {
                    "🔴 已禁用"
                } else {
                    "🟢 已启用"
                };
                let btn_color = if is_muted {
                    colors::ERROR
                } else {
                    colors::SUCCESS
                };

                if ui
                    .add(egui::Button::new(btn_text).fill(btn_color))
                    .clicked()
                {
                    if let Err(e) = event_sender.send(AppEvent::ToggleMute(device_name)) {
                        error!("Failed to send toggle event: {}", e);
                    }
                }
            });
        } else {
            EmojiLabel::new("❌ 此设备没有可用的控制选项").show(ui);
        }

        // Device info
        ui.separator();
        ui.horizontal(|ui| {
            EmojiLabel::new(format!("📋 类型: {:?}", device.device_type)).show(ui);
            EmojiLabel::new(format!(
                "📹 控制: {}",
                if device.has_volume_control && device.has_switch_control {
                    "音量+开关"
                } else if device.has_volume_control {
                    "仅音量"
                } else if device.has_switch_control {
                    "仅开关"
                } else {
                    "无"
                }
            ))
            .show(ui);
        });
    }
}

impl Default for VolumeControlWindow {
    fn default() -> Self {
        Self::new()
    }
}
