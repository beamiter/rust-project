use egui::{Button, Color32};
use egui_plot::{Line, Plot, PlotPoints};

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

const PER_CORE_POINTS_THRESHOLD: usize = 32;

pub struct CpuModule;

impl CpuModule {
    pub fn new() -> Self {
        Self
    }

    fn get_cpu_color(usage: f64) -> Color32 {
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

    fn get_average_cpu_color(cpu_data: &[f64]) -> Color32 {
        if cpu_data.is_empty() {
            return colors::CPU_LOW;
        }
        let avg = cpu_data.iter().sum::<f64>() / cpu_data.len() as f64;
        Self::get_cpu_color(avg)
    }
}

impl BarModule for CpuModule {
    fn id(&self) -> &str {
        "cpu"
    }

    fn name(&self) -> &str {
        "CPU"
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, state: &mut AppState, _anim: &mut AnimationState) {
        // Ensure color cache
        if state.color_cache.is_empty() {
            state.color_cache = (0..=100)
                .map(|i| Self::get_cpu_color(i as f64 / 100.0))
                .collect();
        }

        let reset_view = ui.add(Button::new("🔄"));

        if let Some(snapshot) = state.system_monitor.get_snapshot() {
            let cpu_color = Self::get_cpu_color(snapshot.cpu_average as f64 / 100.0);
            ui.label(
                egui::RichText::new(format!("{}%", snapshot.cpu_average as i32)).color(cpu_color),
            );

            if snapshot.cpu_average > 0.8 * 100.0 {
                ui.label(egui::RichText::new("🔥").color(colors::WARNING));
            }
        }

        let cpu_data = state.get_cpu_chart_data();
        if cpu_data.is_empty() {
            return;
        }

        let available_width = ui.available_width();
        let chart_height = ui.available_height();

        let mut plot = Plot::new("cpu_usage_chart")
            .include_y(0.0)
            .include_y(1.2)
            .x_axis_formatter(|_, _| String::new())
            .y_axis_formatter(|_, _| String::new())
            .show_axes([false, false])
            .show_background(false)
            .width(available_width)
            .height(chart_height);

        if reset_view.clicked() {
            plot = plot.reset();
        }

        plot.show(ui, |plot_ui| {
            let plot_points: Vec<[f64; 2]> = cpu_data
                .iter()
                .enumerate()
                .map(|(i, &usage)| [i as f64, usage])
                .collect();

            if !plot_points.is_empty() {
                let line = Line::new("CPU Usage", PlotPoints::from(plot_points))
                    .color(Self::get_average_cpu_color(&cpu_data))
                    .width(1.0);
                plot_ui.line(line);

                if cpu_data.len() <= PER_CORE_POINTS_THRESHOLD {
                    for (core_idx, &usage) in cpu_data.iter().enumerate() {
                        let color = Self::get_cpu_color(usage);
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
                }

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
}
