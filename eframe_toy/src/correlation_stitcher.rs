use image::{DynamicImage, GenericImageView, ImageBuffer};
use std::error::Error;
use std::path::PathBuf;

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

    /// 从目录中读取所有图像
    fn read_images(&self, image_paths: Vec<PathBuf>) -> Result<Vec<DynamicImage>, Box<dyn Error>> {
        let mut images = Vec::new();
        for path in image_paths {
            println!("读取图像: {:?}", path);
            let img = image::open(&path)?;
            images.push(img);
        }

        Ok(images)
    }

    /// 计算两个图像区域的相似度（使用均方误差）
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

        // 计算均方误差 (MSE)
        let mut sum_squared_diff = 0.0;
        let mut pixel_count = 0;

        for (x, y, p1) in gray1.enumerate_pixels() {
            if let Some(p2) = gray2.get_pixel_checked(x, y) {
                let diff = p1.0[0] as f64 - p2.0[0] as f64;
                sum_squared_diff += diff * diff;
                pixel_count += 1;
            }
        }

        if pixel_count == 0 {
            return 0.0;
        }

        let mse = sum_squared_diff / pixel_count as f64;

        // 转换 MSE 为相似度分数 (0-1)，其中 1 表示完全相同
        let max_possible_mse = 255.0 * 255.0; // 灰度值差异的最大可能平方
        1.0 - (mse / max_possible_mse)
    }

    /// 找到两个图像之间的最佳重叠区域
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

        let mut best_offset = self.y_offset;
        let mut best_similarity = 0.0;

        // 尝试不同的偏移量，找到最佳匹配
        for offset in min_offset..=max_offset {
            if offset >= height1 {
                continue;
            }

            // 计算当前偏移下的重叠区域相似度
            let similarity = self.calculate_similarity(
                img1,
                height1 - offset,
                img2,
                0,
                offset.min(window_height),
            );

            if similarity > best_similarity {
                best_similarity = similarity;
                best_offset = offset;
            }
        }

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

    /// 拼接图像序列
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

        // 计算每对相邻图像的重叠区域
        let mut overlaps = Vec::new();

        for i in 0..images.len() - 1 {
            let overlap = self.find_overlap(&images[i], &images[i + 1]);
            overlaps.push(overlap);

            // 累加总高度，考虑重叠区域
            if i == images.len() - 2 {
                total_height += images[i + 1].height() - overlap;
            } else {
                total_height += self.y_offset;
            }
        }

        println!("创建 {}x{} 的输出图像", width, total_height);

        // 创建输出图像
        let mut output = ImageBuffer::new(width, total_height);

        // 复制第一张图像
        for y in 0..images[0].height() {
            for x in 0..width {
                let pixel = images[0].get_pixel(x, y);
                output.put_pixel(x, y, pixel);
            }
        }

        // 当前写入位置
        let mut y_pos = images[0].height();

        // 拼接剩余图像
        for i in 1..images.len() {
            let overlap = overlaps[i - 1];
            let img = &images[i];

            // 跳过重叠区域
            for y in overlap..img.height() {
                for x in 0..width {
                    let pixel = img.get_pixel(x, y);

                    if y_pos + y - overlap < total_height {
                        output.put_pixel(x, y_pos + y - overlap, pixel);
                    }
                }
            }

            y_pos += img.height() - overlap;
        }

        Ok(DynamicImage::ImageRgba8(output))
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
