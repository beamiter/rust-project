use std::process::Command;

pub struct ScreenSelection {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl ScreenSelection {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn left_x(&self) -> i32 {
        self.x
    }

    pub fn top_y(&self) -> i32 {
        self.y
    }

    pub fn right_x(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom_y(&self) -> i32 {
        self.y + self.height
    }

    pub fn from_slop() -> Result<Self, Box<dyn std::error::Error>> {
        let output = Command::new("slop").arg("-f").arg("%x %y %w %h").output()?;

        if !output.status.success() {
            return Err("slop command failed".into());
        }

        let stdout = String::from_utf8(output.stdout)?;
        let coords: Vec<i32> = stdout
            .trim()
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();

        if coords.len() < 4 {
            return Err("Failed to parse slop output".into());
        }

        Ok(Self::new(coords[0], coords[1], coords[2], coords[3]))
    }

    pub fn capture_screenshot(&self, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("scrot")
            .arg("-a")
            .arg(format!(
                "{},{},{},{}",
                self.x, self.y, self.width, self.height
            ))
            .arg(output_path)
            .status()?;

        if !status.success() {
            return Err("Screenshot command failed".into());
        }

        Ok(())
    }
}

// #[cfg(test)]
// mod tests {
//     use crate::screen_selection::ScreenSelection;
//
//     #[test]
//     fn it_work() -> Result<(), Box<dyn std::error::Error>> {
//         let selection = ScreenSelection::from_slop()?;
//
//         println!("选区左上角坐标: ({}, {})", selection.x, selection.y);
//         println!("选区宽度: {}, 高度: {}", selection.width, selection.height);
//         println!(
//             "选区右下角坐标: ({}, {})",
//             selection.right_x(),
//             selection.bottom_y()
//         );
//
//         selection.capture_screenshot("screenshot.png")?;
//
//         println!("截图已保存到 screenshot.png");
//
//         Ok(())
//     }
// }
