//! System information display component

use crate::app::state::AppState;
use crate::constants::colors;
use crate::utils::RollingAverage;
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};
use egui_twemoji::EmojiLabel;

/// System information panel component
#[allow(dead_code)]
pub struct SystemInfoPanel {
    color_cache: Vec<Color32>,
    chart_data: RollingAverage,
}

impl SystemInfoPanel {
    pub fn new() -> Self {
        Self {
            color_cache: Vec::new(),
            chart_data: RollingAverage::new(60),
        }
    }

    /// Draw system information panel
    pub fn draw(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        // Ensure color cache is initialized
        self.ensure_color_cache();

        // Memory information
        self.draw_memory_info(ui, app_state);

        // CPU chart
        self.draw_cpu_chart(ui, app_state);
    }

    fn draw_memory_info(&self, ui: &mut egui::Ui, app_state: &AppState) {
        let (available_gb, used_gb) = app_state.get_memory_display_info();
        let padding = 2.5;

        // Available memory
        ui.label(
            egui::RichText::new(format!("{:.1}G", available_gb)).color(colors::MEMORY_AVAILABLE),
        );
        ui.add_space(padding);

        // Used memory
        ui.label(egui::RichText::new(format!("{:.1}G", used_gb)).color(colors::MEMORY_USED));
        ui.add_space(padding);

        // Memory warning indicator
        if let Some(snapshot) = app_state.system_monitor.get_snapshot() {
            if snapshot.memory_usage_percent
                > app_state.config.system.memory_warning_threshold * 100.0
            {
                EmojiLabel::new("⚠️").show(ui);
                ui.add_space(padding);
            }
        }
        ui.separator();
    }

    fn draw_cpu_chart(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        // Reset button
        let reset_view = EmojiLabel::new("🔄").show(ui).clicked();

        // CPU usage indicator
        if let Some(snapshot) = app_state.system_monitor.get_snapshot() {
            let cpu_color = self.get_cpu_color(snapshot.cpu_average as f64 / 100.0);
            ui.label(
                egui::RichText::new(format!("{}%", snapshot.cpu_average as i32)).color(cpu_color),
            );

            // CPU warning indicator
            if snapshot.cpu_average > app_state.config.system.cpu_warning_threshold * 100.0 {
                ui.label(egui::RichText::new("🔥").color(colors::WARNING));
            }
        }

        let cpu_data = app_state.get_cpu_chart_data();
        if cpu_data.is_empty() {
            return;
        }

        let available_width = ui.available_width();
        let chart_height = ui.available_height();
        let chart_width = available_width;

        let mut plot = Plot::new("cpu_usage_chart")
            .include_y(0.0)
            .include_y(1.2)
            .x_axis_formatter(|_, _| String::new())
            .y_axis_formatter(|_, _| String::new())
            .show_axes([false, false])
            .show_background(false)
            .width(chart_width)
            .height(chart_height);
        if reset_view {
            plot = plot.reset();
        }

        plot.show(ui, |plot_ui| {
            // Create plot points for all CPU cores
            let plot_points: Vec<[f64; 2]> = cpu_data
                .iter()
                .enumerate()
                .map(|(i, &usage)| [i as f64, usage])
                .collect();

            if !plot_points.is_empty() {
                let line = Line::new("CPU Usage", PlotPoints::from(plot_points))
                    .color(self.get_average_cpu_color(&cpu_data))
                    .width(1.0);
                plot_ui.line(line);

                // Draw individual CPU core points with different colors
                for (core_idx, &usage) in cpu_data.iter().enumerate() {
                    let color = self.get_cpu_color(usage);
                    let points = vec![[core_idx as f64, usage]];

                    let core_point = egui_plot::Points::new(
                        format!("Core {}", core_idx),
                        PlotPoints::from(points),
                    )
                    .color(color)
                    .radius(2.0)
                    .shape(egui_plot::MarkerShape::Circle);

                    plot_ui.points(core_point);
                }

                // Draw average line if we have multiple cores
                if cpu_data.len() > 1 {
                    let avg_usage = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
                    let avg_points: Vec<[f64; 2]> =
                        (0..cpu_data.len()).map(|i| [i as f64, avg_usage]).collect();

                    let avg_line = Line::new("Average", PlotPoints::from(avg_points))
                        .color(Color32::WHITE)
                        .width(1.0)
                        .style(egui_plot::LineStyle::Dashed { length: 5.0 });

                    plot_ui.line(avg_line);
                }
            }
        });
    }

    fn get_cpu_color(&self, usage: f64) -> Color32 {
        let usage = usage.clamp(0.0, 1.0);

        if usage < 0.3 {
            colors::CPU_LOW
        } else if usage < 0.6 {
            colors::CPU_MEDIUM
        } else if usage < 0.8 {
            colors::CPU_HIGH
        } else {
            colors::CPU_CRITICAL
        }
    }

    fn get_average_cpu_color(&self, cpu_data: &[f64]) -> Color32 {
        if cpu_data.is_empty() {
            return colors::CPU_LOW;
        }

        let avg_usage = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
        self.get_cpu_color(avg_usage)
    }

    fn ensure_color_cache(&mut self) {
        if self.color_cache.is_empty() {
            self.color_cache = (0..=100)
                .map(|i| {
                    let usage = i as f64 / 100.0;
                    self.get_cpu_color(usage)
                })
                .collect();
        }
    }
}

impl Default for SystemInfoPanel {
    fn default() -> Self {
        Self::new()
    }
}
