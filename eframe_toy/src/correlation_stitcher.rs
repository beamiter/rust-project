use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use rayon::prelude::*;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 用于拼接滚动截图的结构体
pub struct ScrollStitcher {
    /// 已知的滚动偏移量（像素）
    y_offset: u32,
    /// 相似度阈值，用于确定重叠区域
    similarity_threshold: f64,
    /// 是否使用已知的滚动偏移量
    use_fixed_offset: bool,
}

impl ScrollStitcher {
    /// 创建新的 ScrollStitcher 实例
    pub fn new(y_offset: u32, similarity_threshold: f64, use_fixed_offset: bool) -> Self {
        Self {
            y_offset,
            similarity_threshold,
            use_fixed_offset,
        }
    }

    /// 从目录中读取所有图像 - 并行化读取
    fn read_images(&self, image_paths: Vec<PathBuf>) -> Result<Vec<DynamicImage>, Box<dyn Error>> {
        println!("并行读取 {} 个图像文件...", image_paths.len());

        // 使用 rayon 并行读取图像
        let images: Result<Vec<_>, _> = image_paths
            .par_iter()
            .map(|path| {
                println!("读取图像: {:?}", path);
                image::open(path).map_err(|e| Box::new(e) as Box<dyn Error + Send>)
            })
            .collect();

        images.map_err(|e| e as Box<dyn Error>)
    }

    /// 计算两个图像区域的相似度（使用均方误差）- 优化计算
    fn calculate_similarity(
        &self,
        img1: &DynamicImage,
        y1: u32,
        img2: &DynamicImage,
        y2: u32,
        height: u32,
    ) -> f64 {
        // 确保两个图像宽度相同
        if img1.width() != img2.width() {
            return 0.0;
        }

        let width = img1.width();

        // 将两个区域转换为灰度图进行比较
        let region1 = img1.crop_imm(0, y1, width, height);
        let region2 = img2.crop_imm(0, y2, width, height);

        let gray1 = region1.to_luma8();
        let gray2 = region2.to_luma8();

        // 使用平行迭代器计算均方误差 (MSE)
        let sum_squared_diff: f64 = gray1
            .enumerate_pixels()
            .filter_map(|(x, y, p1)| {
                gray2.get_pixel_checked(x, y).map(|p2| {
                    let diff = p1.0[0] as f64 - p2.0[0] as f64;
                    diff * diff
                })
            })
            .sum();

        let pixel_count = (width * height) as f64;
        if pixel_count == 0.0 {
            return 0.0;
        }

        let mse = sum_squared_diff / pixel_count;

        // 转换 MSE 为相似度分数 (0-1)，其中 1 表示完全相同
        let max_possible_mse = 255.0 * 255.0; // 灰度值差异的最大可能平方
        1.0 - (mse / max_possible_mse)
    }

    /// 找到两个图像之间的最佳重叠区域 - 并行搜索
    fn find_overlap(&self, img1: &DynamicImage, img2: &DynamicImage) -> u32 {
        if self.use_fixed_offset {
            return self.y_offset;
        }

        let height1 = img1.height();
        let height2 = img2.height();

        // 搜索范围：从 y_offset/2 到 y_offset*1.5
        let min_offset = (self.y_offset as f64 * 0.5).round() as u32;
        let max_offset = (self.y_offset as f64 * 1.5).round() as u32;

        // 搜索窗口高度
        let window_height = height1.min(height2) / 4;

        // 并行搜索最佳偏移量
        let results: Vec<(u32, f64)> = (min_offset..=max_offset)
            .into_par_iter()
            .filter(|&offset| offset < height1)
            .map(|offset| {
                // 计算当前偏移下的重叠区域相似度
                let similarity = self.calculate_similarity(
                    img1,
                    height1 - offset,
                    img2,
                    0,
                    offset.min(window_height),
                );
                (offset, similarity)
            })
            .collect();

        // 找出相似度最高的偏移量
        let best = results
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((self.y_offset, 0.0));

        let (best_offset, best_similarity) = best;

        println!(
            "检测到最佳重叠: {} 像素 (相似度: {:.2})",
            best_offset, best_similarity
        );

        // 如果最佳相似度低于阈值，使用默认偏移量
        if best_similarity < self.similarity_threshold {
            println!("相似度过低，使用默认偏移量: {}", self.y_offset);
            return self.y_offset;
        }

        best_offset
    }

    /// 拼接图像序列 - 优化的多线程实现
    fn stitch_images(&self, images: &[DynamicImage]) -> Result<DynamicImage, Box<dyn Error>> {
        if images.is_empty() {
            return Err("没有输入图像".into());
        }

        if images.len() == 1 {
            return Ok(images[0].clone());
        }

        // 获取第一张图像的尺寸
        let width = images[0].width();
        let mut total_height = images[0].height();

        println!("计算图像重叠区域...");

        // 并行计算每对相邻图像的重叠区域
        let overlaps: Vec<u32> = (0..images.len() - 1)
            .into_par_iter()
            .map(|i| self.find_overlap(&images[i], &images[i + 1]))
            .collect();

        // 计算总高度
        for i in 0..images.len() - 1 {
            if i == images.len() - 2 {
                total_height += images[i + 1].height() - overlaps[i];
            } else {
                total_height += images[i + 1].height() - overlaps[i];
            }
        }

        println!("创建 {}x{} 的输出图像", width, total_height);

        // 创建输出图像
        let output = Arc::new(Mutex::new(ImageBuffer::from_fn(
            width,
            total_height,
            |_, _| Rgba([255, 255, 255, 255]),
        )));

        println!("开始并行拼接图像...");

        // 计算每张图像的起始位置
        let mut positions = Vec::with_capacity(images.len());
        let mut current_pos = 0;
        positions.push(current_pos);

        for i in 0..images.len() - 1 {
            current_pos += images[i].height() - overlaps[i];
            positions.push(current_pos);
        }

        // 并行处理每张图像
        images.par_iter().enumerate().for_each(|(idx, img)| {
            let y_pos = positions[idx];
            let mut output_lock = output.lock().unwrap();

            // 复制图像像素
            for y in 0..img.height() {
                let output_y = y_pos + y;
                if output_y >= total_height {
                    continue;
                }

                for x in 0..width {
                    let pixel = img.get_pixel(x, y);
                    output_lock.put_pixel(x, output_y, pixel);
                }
            }
        });

        // 解锁并返回结果
        let final_image = Arc::try_unwrap(output)
            .expect("引用计数错误")
            .into_inner()
            .expect("互斥锁错误");

        Ok(DynamicImage::ImageRgba8(final_image))
    }

    /// 处理目录中的所有图像并保存结果
    pub fn process_directory(
        &self,
        image_paths: Vec<PathBuf>,
        output_file: PathBuf,
    ) -> Result<(), Box<dyn Error>> {
        // 读取所有图像
        let images = self.read_images(image_paths)?;
        if images.is_empty() {
            return Err("未找到图像文件".into());
        }
        println!("找到 {} 张图像", images.len());

        // 拼接图像
        let stitched = self.stitch_images(&images)?;

        // 保存结果
        println!("保存拼接图像到 {:?}", output_file);
        stitched.save(output_file)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    #[test]
    fn it_work() -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

