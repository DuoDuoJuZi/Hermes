/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-22
 */
use image::{imageops::FilterType, GenericImageView};
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::error::Error;

/// 图片矩阵数据结构用于 STM32 等单片机的底层绘制接口
pub struct ImageMatrix {
    pub width: u32,
    pub height: u32,
    pub rgb_data: Vec<u8>,
}

/// 点阵字体数据结构用于预留给外部设备的展示使用
pub struct TextMatrix {
    pub width: usize,
    pub height: usize,
    pub pixel_data: Vec<u8>,
}

/// 异步获取网易云封面并解析降采样为像素矩阵数据
pub async fn fetch_cover_matrix(pic_url: &str) -> Result<ImageMatrix, Box<dyn Error>> {
    let img_bytes = reqwest::get(pic_url).await?.bytes().await?;
    let img = image::load_from_memory(&img_bytes)?;
    let resized = img.resize_exact(40, 40, FilterType::Triangle);

    
    let mut rgb_data = Vec::with_capacity((resized.width() * resized.height() * 3) as usize);
    for y in 0..resized.height() {
        for x in 0..resized.width() {
            let pixel = resized.get_pixel(x, y);
            rgb_data.push(pixel[0]);
            rgb_data.push(pixel[1]);
            rgb_data.push(pixel[2]);
        }
    }
    
    Ok(ImageMatrix {
        width: resized.width(),
        height: resized.height(),
        rgb_data,
    })
}

/// 遍历像素根据 RGB 转义序列直接在控制台彩色打印输出
pub fn print_cover_to_console(matrix: &ImageMatrix) {
    println!("--- 专辑封面预览 ---");
    let mut idx = 0;
    for _ in 0..matrix.height {
        let mut line = String::new();
        for _ in 0..matrix.width {
            let r = matrix.rgb_data[idx];
            let g = matrix.rgb_data[idx + 1];
            let b = matrix.rgb_data[idx + 2];
            idx += 3;
            line.push_str(&format!("\x1b[38;2;{};{};{}m██\x1b[0m", r, g, b));
        }
        println!("{}", line);
    }
}

/// 快捷方法用于合并网络下载与控制台封面的打印任务
pub async fn fetch_and_print_cover(pic_url: &str) -> Result<(), Box<dyn Error>> {
    let matrix = fetch_cover_matrix(pic_url).await?;
    print_cover_to_console(&matrix);
    Ok(())
}

/// 生成包含字体点阵的二维数组以便将其发送给单片机渲染展示
pub fn generate_text_matrix(text: &str) -> Option<TextMatrix> {
    let font_data = std::fs::read(r#"C:\Windows\Fonts\msyh.ttc"#).ok()?;
    let font = Font::try_from_vec_and_index(font_data, 0)?;
    let height = 24.0;
    let scale = Scale::uniform(height);
    let v_metrics = font.v_metrics(scale);
    let offset = point(0.0, v_metrics.ascent);
    let glyphs: Vec<PositionedGlyph> = font.layout(text, scale, offset).collect();
    
    if glyphs.is_empty() {
        return None;
    }
    
    let width = glyphs.last()?.pixel_bounding_box()?.max.x as usize;
    let height_ceil = height.ceil() as usize;
    let mut pixel_data = vec![0u8; width * height_ceil];
    
    for g in glyphs {
        if let Some(bb) = g.pixel_bounding_box() {
            g.draw(|x, y, v| {
                let px = (bb.min.x + x as i32) as usize;
                let py = (bb.min.y + y as i32) as usize;
                if px < width && py < height_ceil {
                    pixel_data[py * width + px] = (v * 255.0) as u8;
                }
            });
        }
    }
    
    Some(TextMatrix {
        width,
        height: height_ceil,
        pixel_data,
    })
}

/// 通过控制台直观地还原单片机上具体的点阵输出效果
pub fn print_text_matrix(matrix: &TextMatrix, text: &str) {
    println!("--- 歌词点阵预览: [{}] ---", text);
    for y in 0..matrix.height {
        let mut line = String::new();
        for x in 0..matrix.width {
            let pixel_val = matrix.pixel_data[y * matrix.width + x];
            if pixel_val > 128 {
                line.push_str("██");
            } else {
                line.push_str("  ");
            }
        }
        if !line.trim().is_empty() {
            println!("{}", line);
        }
    }
}

/// 对生成的矩阵数据进行封装执行从而实现点阵终端的打印
pub fn render_text_to_console(text: &str) {
    if let Some(matrix) = generate_text_matrix(text) {
        print_text_matrix(&matrix, text);
    }
}
