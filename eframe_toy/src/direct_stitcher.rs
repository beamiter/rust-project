use image::{DynamicImage, GenericImageView, RgbaImage};
use std::error::Error;
use std::path::PathBuf;

/// 用于拼接滚动截图的结构体
pub struct ScrollStitcher {
    /// 滚动偏移量（像素）
    y_offset: u32,
    /// 偏移量的搜索范围（百分比）
    offset_range: f64,
}

impl ScrollStitcher {
    /// 创建新的 ScrollStitcher 实例
    pub fn new(y_offset: u32, offset_range: f64) -> Self {
        Self {
            y_offset,
            offset_range,
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

    /// 检测两个图像之间的重叠区域
    fn detect_overlap(&self, img1: &DynamicImage, img2: &DynamicImage) -> u32 {
        let height1 = img1.height();
        let height2 = img2.height();
        let width = img1.width();

        // 计算搜索范围
        let min_offset = (self.y_offset as f64 * (1.0 - self.offset_range)).round() as u32;
        let max_offset = (self.y_offset as f64 * (1.0 + self.offset_range)).round() as u32;

        // 限制搜索范围不超过图像高度
        let min_offset = min_offset.min(height1).min(height2);
        let max_offset = max_offset.min(height1).min(height2);

        // 在搜索范围内查找完全匹配的区域
        for offset in min_offset..=max_offset {
            // 检查图像1底部的offset像素和图像2顶部的offset像素是否完全匹配
            let mut match_found = true;

            'pixel_check: for y in 0..offset {
                for x in 0..width {
                    let y1 = height1 - offset + y;
                    let y2 = y;

                    let pixel1 = img1.get_pixel(x, y1);
                    let pixel2 = img2.get_pixel(x, y2);

                    // 如果像素不完全相同，这个偏移量不是精确匹配
                    if pixel1 != pixel2 {
                        match_found = false;
                        break 'pixel_check;
                    }
                }
            }

            if match_found {
                println!("找到精确重叠: {} 像素", offset);
                return offset;
            }
        }

        // 如果没有找到精确匹配，返回默认偏移量
        println!("未找到精确重叠，使用默认偏移量: {}", self.y_offset);
        self.y_offset
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

        // 计算每对相邻图像的重叠区域
        let mut overlaps = Vec::new();

        for i in 0..images.len() - 1 {
            let overlap = self.detect_overlap(&images[i], &images[i + 1]);
            overlaps.push(overlap);
        }

        // 计算输出图像的总高度
        let mut total_height = images[0].height();
        for i in 1..images.len() {
            total_height += images[i].height() - overlaps[i - 1];
        }

        println!("创建 {}x{} 的输出图像", width, total_height);

        // 创建输出图像
        let mut output = RgbaImage::new(width, total_height);

        // 复制第一张图像
        for y in 0..images[0].height() {
            for x in 0..width {
                output.put_pixel(x, y, images[0].get_pixel(x, y));
            }
        }

        // 当前写入位置
        let mut y_pos = images[0].height();

        // 拼接剩余图像
        for i in 1..images.len() {
            let overlap = overlaps[i - 1];
            let img = &images[i];

            // 跳过重叠区域，只复制非重叠部分
            for y in overlap..img.height() {
                for x in 0..width {
                    if y_pos + y - overlap < total_height {
                        output.put_pixel(x, y_pos + y - overlap, img.get_pixel(x, y));
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

#[test]
fn it_work() -> Result<(), Box<dyn Error>> {
    Ok(())
}
