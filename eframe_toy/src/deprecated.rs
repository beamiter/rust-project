use crate::ImageProcessor;
use device_query::DeviceQuery;
use enigo::Mouse;
use image::ImageReader;
use std::process::Command;
use std::thread;
use std::time::Duration;

impl ImageProcessor {
    pub fn load_image_from_path(
        &mut self,
        path: &std::path::Path,
        ctx: &egui::Context,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 清除之前的纹理
        self.textures.clear();

        // 加载图像
        let image = ImageReader::open(path)?.decode()?;
        let width = image.width();
        let height = image.height();

        // egui 纹理尺寸限制
        const MAX_TEXTURE_HEIGHT: u32 = 16000;

        if height <= MAX_TEXTURE_HEIGHT {
            // 图像高度在限制范围内，直接加载为单一纹理
            let texture = ctx.load_texture(
                "image_single",
                egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    image.to_rgba8().as_raw(),
                ),
                Default::default(),
            );
            self.textures.push(texture);
        } else {
            // 图像太高，需要分块处理
            let num_blocks = (height + MAX_TEXTURE_HEIGHT - 1) / MAX_TEXTURE_HEIGHT;
            println!(
                "图像高度 {} 超过纹理限制 {}，分割为 {} 个块",
                height, MAX_TEXTURE_HEIGHT, num_blocks
            );

            // 处理每个块
            for i in 0..num_blocks {
                let start_y = i * MAX_TEXTURE_HEIGHT;
                let block_height = MAX_TEXTURE_HEIGHT.min(height - start_y);

                // 裁剪当前块
                let block = image.crop_imm(0, start_y, width, block_height);

                // 加载为纹理
                let texture = ctx.load_texture(
                    format!("image_block_{}", i),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, block_height as usize],
                        block.to_rgba8().as_raw(),
                    ),
                    Default::default(),
                );

                self.textures.push(texture);
            }
        }

        Ok(())
    }
    pub fn scroll_display(&mut self, ui: &mut egui::Ui) {
        if self.textures.is_empty() {
            ui.label("no images loaded");
            return;
        }

        // 创建垂直滚动区域
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // 垂直堆叠所有纹理块
                for texture in &self.textures {
                    ui.image(texture);
                    // use egui::Widget;
                    // egui::Image::new(texture).shrink_to_fit().ui(ui);
                }
            });
    }

    #[allow(dead_code)]
    fn capture_screen_area(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        output_path: &str,
    ) -> Result<(), String> {
        let output = Command::new("scrot")
            .args(&[
                "-a",
                &format!("{},{},{},{}", x, y, width, height),
                output_path,
            ])
            .output()
            .map_err(|e| format!("执行scrot失败: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(format!("scrot执行出错: {}", error))
        }
    }
    fn display_results(
        &mut self,
        points: &Vec<(i32, i32)>,
    ) -> Result<(i32, i32), Box<dyn std::error::Error>> {
        if points.len() < 2 {
            return Ok((0, 0));
        }
        println!("\n=== 记录结果 ===");
        println!("pnt1: ({}, {})", points[0].0, points[0].1);
        println!("pnt2: ({}, {})", points[1].0, points[1].1);
        let dx = points[1].0 - points[0].0;
        let dy = points[1].1 - points[0].1;
        // assert!(dx >= 0);
        // assert!(dy >= 0);
        let distance = ((dx * dx + dy * dy) as f64).sqrt();
        println!("dx: {} 像素", dx.abs());
        println!("dy: {} 像素", dy.abs());
        println!("distance: {:.2} 像素", distance);
        Ok((dx.abs(), dy.abs()))
    }
    #[allow(dead_code)]
    fn select_positions(
        &mut self,
        mut corner_points: Vec<(i32, i32)>,
    ) -> Result<(i32, i32), Box<dyn std::error::Error>> {
        let mut left_button_was_pressed = false;
        corner_points.clear();
        loop {
            let keys = self.device_state.get_keys();
            let mouse = self.device_state.get_mouse();

            if keys.contains(&device_query::Keycode::Escape) {
                println!("cancel");
                break;
            }
            let left_button_pressed = mouse.button_pressed[1];
            if left_button_pressed && !left_button_was_pressed {
                let coords = mouse.coords;
                corner_points.push(coords);
                println!("pnt #{}: ({}, {})", corner_points.len(), coords.0, coords.1);
                if corner_points.len() >= 2 {
                    return self.display_results(&corner_points);
                }
            }
            left_button_was_pressed = left_button_pressed;
            thread::sleep(Duration::from_millis(10));
        }
        Ok((0, 0))
    }
    pub fn verify_scroll_pixel(&mut self) -> Result<(i32, i32), Box<dyn std::error::Error>> {
        let mut corner_points: Vec<(i32, i32)> = Vec::new();
        let mut left_button_was_pressed = false;
        let mut scroll_once = false;
        loop {
            let keys = self.device_state.get_keys();
            let mouse = self.device_state.get_mouse();

            if keys.contains(&device_query::Keycode::Escape) {
                println!("cancel");
                break;
            }
            if corner_points.len() == 1 && !scroll_once {
                scroll_once = true;
                self.enigo
                    .scroll(self.user_info.scroll_num, enigo::Axis::Vertical)
                    .unwrap();
                thread::sleep(Duration::from_millis(10));
            }
            let left_button_pressed = mouse.button_pressed[1];
            if left_button_pressed && !left_button_was_pressed {
                let coords = mouse.coords;
                corner_points.push(coords);
                println!("pnt #{}: ({}, {})", corner_points.len(), coords.0, coords.1);
                if corner_points.len() >= 2 {
                    return self.display_results(&corner_points);
                }
            }
            left_button_was_pressed = left_button_pressed;
            thread::sleep(Duration::from_millis(10));
        }
        Ok((0, 0))
    }
}
