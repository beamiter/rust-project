use egui::{Align, Button, Label, Layout};
use log::error;

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::{colors, icons};
use xbar_core::audio_manager::AudioDevice;

use super::BarModule;

pub struct AudioModule;

impl AudioModule {
    pub fn new() -> Self {
        Self
    }
}

impl BarModule for AudioModule {
    fn id(&self) -> &str {
        "audio"
    }

    fn name(&self) -> &str {
        "Audio"
    }

    fn has_popup(&self) -> bool {
        true
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, _anim: &mut AnimationState) {
        let (volume_icon, tooltip) = if let Some(device) = state.get_master_audio_device() {
            let icon = if device.is_muted || device.volume == 0 {
                icons::VOLUME_MUTED
            } else if device.volume < 30 {
                icons::VOLUME_LOW
            } else if device.volume < 70 {
                icons::VOLUME_MEDIUM
            } else {
                icons::VOLUME_HIGH
            };

            let tooltip = format!(
                "{}: {}%{}",
                device.description,
                device.volume,
                if device.is_muted { " (muted)" } else { "" }
            );

            (icon, tooltip)
        } else {
            (icons::VOLUME_MUTED, "No audio device".to_string())
        };

        let label_response = ui.add(Button::new(volume_icon));
        if label_response.clicked() {
            state.ui_state.toggle_volume_window();
        }
        label_response.on_hover_text(tooltip);
    }

    fn render_popup(&mut self, ctx: &egui::Context, state: &mut AppState) {
        if !state.ui_state.volume_window.open {
            return;
        }

        let mut window_open = true;

        egui::Window::new("🔊 Volume Control")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .default_pos(
                state
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
                if let Some(rect) = ctx.memory(|mem| mem.area_rect(ui.id())) {
                    state.ui_state.volume_window.position = Some(rect.left_top());
                }

                draw_volume_content(ui, state);

                ui.horizontal(|ui| {
                    if ui.button("🔧 Advanced Mixer").clicked() {
                        let _ = std::process::Command::new("terminator")
                            .args(["-e", "alsamixer"])
                            .spawn();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✖ Close").clicked() {
                            state.ui_state.toggle_volume_window();
                        }
                    });
                });
            });

        if !window_open || ctx.input(|i| i.viewport().close_requested()) {
            state.ui_state.toggle_volume_window();
        }
    }
}

fn draw_volume_content(ui: &mut egui::Ui, state: &mut AppState) {
    let devices: Vec<AudioDevice> = state.audio_manager.get_devices().to_vec();

    if devices.is_empty() {
        ui.add(Label::new("❌ No controllable audio device found"));
        return;
    }

    let controllable_devices: Vec<(usize, AudioDevice)> = devices
        .into_iter()
        .enumerate()
        .filter(|(_, d)| d.has_volume_control || d.has_switch_control)
        .collect();

    if controllable_devices.is_empty() {
        ui.add(Label::new("❌ No controllable audio device found"));
        return;
    }

    draw_device_selector(ui, state, &controllable_devices);
    ui.add_space(10.0);

    if let Some((_, device)) =
        controllable_devices.get(state.ui_state.volume_window.selected_device)
    {
        draw_device_controls(ui, state, device);
    }
}

fn draw_device_selector(
    ui: &mut egui::Ui,
    state: &mut AppState,
    controllable_devices: &[(usize, AudioDevice)],
) {
    ui.horizontal(|ui| {
        ui.add(Label::new("🎵 Device:"));

        if state.ui_state.volume_window.selected_device >= controllable_devices.len() {
            state.ui_state.volume_window.selected_device = 0;
        }

        let current_selection =
            &controllable_devices[state.ui_state.volume_window.selected_device];

        egui::ComboBox::from_id_salt("audio_device_selector")
            .selected_text(&current_selection.1.description)
            .width(200.0)
            .show_ui(ui, |ui| {
                for (idx, (_, device)) in controllable_devices.iter().enumerate() {
                    if ui
                        .selectable_label(
                            state.ui_state.volume_window.selected_device == idx,
                            &device.description,
                        )
                        .clicked()
                    {
                        state.ui_state.volume_window.selected_device = idx;
                    }
                }
            });
    });
}

fn draw_device_controls(ui: &mut egui::Ui, state: &mut AppState, device: &AudioDevice) {
    let device_name = device.name.clone();
    let mut current_volume = device.volume;
    let is_muted = device.is_muted;

    if device.has_volume_control {
        ui.horizontal(|ui| {
            ui.add(Label::new("🔊 Volume:"));

            if device.has_switch_control {
                let mute_icon = if is_muted {
                    icons::VOLUME_MUTED
                } else {
                    icons::VOLUME_HIGH
                };
                let mute_btn = ui.button(mute_icon);

                if mute_btn.clicked() {
                    if let Err(e) = state.audio_manager.toggle_mute(&device_name) {
                        error!("Failed to toggle mute: {}", e);
                    }
                }

                mute_btn.on_hover_text(if is_muted { "Unmute" } else { "Mute" });
            }

            ui.label(format!("{}%", current_volume));
        });

        let slider_response = ui.add(
            egui::Slider::new(&mut current_volume, 0..=100)
                .show_value(false)
                .text(""),
        );

        if slider_response.changed()
            && state
                .ui_state
                .volume_window
                .should_apply_volume_change()
        {
            if let Err(e) =
                state
                    .audio_manager
                    .set_volume(&device_name, current_volume, is_muted)
            {
                error!("Failed to set volume: {}", e);
            }
        }
    } else if device.has_switch_control {
        ui.horizontal(|ui| {
            let btn_text = if is_muted {
                "🔴 Disabled"
            } else {
                "🟢 Enabled"
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
                if let Err(e) = state.audio_manager.toggle_mute(&device_name) {
                    error!("Failed to toggle mute: {}", e);
                }
            }
        });
    } else {
        ui.add(Label::new("❌ No available controls for this device"));
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.add(Label::new(format!("📋 Type: {:?}", device.device_type)));
        ui.add(Label::new(format!(
            "📹 Controls: {}",
            if device.has_volume_control && device.has_switch_control {
                "Volume + Switch"
            } else if device.has_volume_control {
                "Volume only"
            } else if device.has_switch_control {
                "Switch only"
            } else {
                "None"
            }
        )));
    });
}
