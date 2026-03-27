/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-22
 */
use image::imageops::FilterType;
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::error::Error;

/// 图像矩阵数据结构，用于 STM32 等单片机的底层绘制接口
pub struct ImageMatrix {
    pub width: u32,
    pub height: u32,
    pub theme_color: (u8, u8, u8),
    pub rgb_data: Vec<u8>,
}

/// 点阵字体数据结构，用于预留给外部设备的展示使用
pub struct TextMatrix {
    pub width: usize,
    pub height: usize,
    pub pixel_data: Vec<u8>,
}

/// 拆分后的文本图层块数据，包含自身坐标与宽高
pub struct TextLayer {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub is_active: u8,
    pub pixel_data: Vec<u8>,
}

/// 异步获取网易云封面并解析降采样为像素矩阵数据
pub async fn fetch_cover_matrix(pic_url: &str) -> Result<ImageMatrix, Box<dyn Error>> {
    let img_bytes = reqwest::get(pic_url).await?.bytes().await?;
    let img = image::load_from_memory(&img_bytes)?.into_rgb8();
    let resized = image::imageops::resize(&img, 200, 200, FilterType::Lanczos3);

    let mut rgb_data = Vec::with_capacity((resized.width() * resized.height() * 3) as usize);
    for y in 0..resized.height() {
        for x in 0..resized.width() {
            let pixel = resized.get_pixel(x, y);
            rgb_data.push(pixel[0]);
            rgb_data.push(pixel[1]);
            rgb_data.push(pixel[2]);
        }
    }

    let mut bins = std::collections::HashMap::new();
    let mut max_count = 0;
    let mut dominant_center = (0, 0, 0);

    for i in (0..rgb_data.len()).step_by(3) {
        let r = rgb_data[i];
        let g = rgb_data[i+1];
        let b = rgb_data[i+2];

        let r_bin = (r >> 3) as u16;
        let g_bin = (g >> 3) as u16;
        let b_bin = (b >> 3) as u16;

        let key = (r_bin << 10) | (g_bin << 5) | b_bin;
        
        let entry = bins.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        entry.1 += r as u32;
        entry.2 += g as u32;
        entry.3 += b as u32;

        if entry.0 > max_count {
            max_count = entry.0;
            dominant_center = (
                (entry.1 / entry.0) as u8,
                (entry.2 / entry.0) as u8,
                (entry.3 / entry.0) as u8
            );
        }
    }

    let luma = 0.299 * (dominant_center.0 as f32) + 0.587 * (dominant_center.1 as f32) + 0.114 * (dominant_center.2 as f32);
    if luma > 80.0 {
        let scale = 80.0 / luma;
        dominant_center.0 = (dominant_center.0 as f32 * scale) as u8;
        dominant_center.1 = (dominant_center.1 as f32 * scale) as u8;
        dominant_center.2 = (dominant_center.2 as f32 * scale) as u8;
    }

    Ok(ImageMatrix {
        width: resized.width(),
        height: resized.height(),
        theme_color: dominant_center,
        rgb_data,
    })
}

/// 遍历像素根据 RGB 转义序列直接在控制台彩色打印输出
pub fn print_cover_to_console(matrix: &ImageMatrix) {
    println!("--- 专辑封面已解析 (控制台预览由于编码问题可能乱码，此处已隐藏) ---");
}

/// 快捷方法用于合并网络下载与控制台封面的打印任务
pub async fn fetch_and_print_cover(pic_url: &str) -> Result<(), Box<dyn Error>> {
    let matrix = fetch_cover_matrix(pic_url).await?;
    print_cover_to_console(&matrix);
    Ok(())
}

/// 生成文本图层，根据字符串列表渲染带缩放及粗体样式的字体点阵层数据
pub fn generate_text_layers(lines: &[String]) -> Option<Vec<TextLayer>> {
    let font_data = std::fs::read(r#"C:\Windows\Fonts\msyh.ttc"#).ok()?;
    let font = Font::try_from_vec_and_index(font_data, 0)?;

    let max_width = 480.0;

    struct GlyphInfo<'a> {
        glyph: PositionedGlyph<'a>,
        alpha_mult: f32,
        is_bold: bool,
    }

    struct LineBlock<'a> {
        glyphs: Vec<GlyphInfo<'a>>,
        height: f32,
        is_active: u8,
    }

    let mut blocks = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let (size, alpha_mult, is_bold, is_active) = if i == 3 {
            (40.0, 1.0, true, 1u8)
        } else {
            (26.0, 1.0, true, 0u8)
        };

        let scale = Scale::uniform(size);
        let v_metrics = font.v_metrics(scale);
        let line_height = size * 1.3;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            blocks.push(LineBlock { glyphs: vec![], height: 0.0, is_active });
            continue;
        }

        let mut block_glyphs = Vec::new();
        let mut current_x = 0.0;
        let mut block_height = line_height;
        let mut current_y = v_metrics.ascent;

        for c in trimmed.chars() {
            let base_glyph = font.glyph(c);
            let scaled_glyph = base_glyph.scaled(scale);
            let h_metrics = scaled_glyph.h_metrics();

            if current_x + h_metrics.advance_width > max_width && current_x > 0.0 {
                current_x = 0.0;
                current_y += line_height;
                block_height += line_height;
            }

            let positioned = scaled_glyph.positioned(point(current_x, current_y));
            current_x += h_metrics.advance_width;

            block_glyphs.push(GlyphInfo {
                glyph: positioned,
                alpha_mult,
                is_bold,
            });
        }

        block_height += size * 0.3;
        blocks.push(LineBlock { glyphs: block_glyphs, height: block_height, is_active });
    }

    if blocks.is_empty() || blocks.len() < 7 {
        return None;
    }

    let top_height = blocks[0].height + blocks[1].height + blocks[2].height;
    let bottom_height = blocks[4].height + blocks[5].height + blocks[6].height;
    let center_height = blocks[3].height;

    let half_offset = top_height.max(bottom_height);

    let start_y_0 = half_offset - top_height;
    let start_y_1 = start_y_0 + blocks[0].height;
    let start_y_2 = start_y_1 + blocks[1].height;
    let start_y_3 = half_offset;
    let start_y_4 = start_y_3 + blocks[3].height;
    let start_y_5 = start_y_4 + blocks[4].height;
    let start_y_6 = start_y_5 + blocks[5].height;

    let y_offsets = [start_y_0, start_y_1, start_y_2, start_y_3, start_y_4, start_y_5, start_y_6];

    let virtual_total_height = half_offset * 2.0 + center_height;
    let screen_y_base = (480.0 - virtual_total_height) / 2.0;

    let mut layers = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        if block.glyphs.is_empty() { continue; }
        
        let mut actual_max_x = 0;
        let mut actual_max_y = 0;
        for g_info in &block.glyphs {
            if let Some(bb) = g_info.glyph.pixel_bounding_box() {
                if bb.max.x > actual_max_x { actual_max_x = bb.max.x; }
                if bb.max.y > actual_max_y { actual_max_y = bb.max.y; }
            }
        }
        
        let actual_width = (actual_max_x as usize).max(1);
        let height_ceil = (actual_max_y as usize).max(1);
        
        let mut pixel_data = vec![0u8; actual_width * height_ceil];
        for info in block.glyphs {
            if let Some(bb) = info.glyph.pixel_bounding_box() {
                info.glyph.draw(|x, y, v| {
                    let mut draw_pixel = |dx: i32| {
                        let px = bb.min.x + x as i32 + dx;
                        let py = bb.min.y + y as i32;
                        if px >= 0 && px < actual_width as i32 && py >= 0 && py < height_ceil as i32 {
                            let idx = (py as usize) * actual_width + (px as usize);
                            let val = (v * 255.0 * info.alpha_mult) as u8;
                            if val > pixel_data[idx] {
                                pixel_data[idx] = val;
                            }
                        }
                    };
                    draw_pixel(0);
                    if info.is_bold {
                        draw_pixel(1);
                    }
                });
            }
        }
        
        let y_float = screen_y_base + y_offsets[i];
        let start_x = 310;

        layers.push(TextLayer {
            x: start_x as i16,
            y: y_float as i16,
            width: actual_width as u16,
            height: height_ceil as u16,
            is_active: block.is_active,
            pixel_data,
        });
    }

    Some(layers)
}

/// 打印文本像素点阵到控制台
pub fn print_text_matrix(matrix: &TextMatrix, text: &str) {}

/// 直接将字符串渲染成像素矩阵并输出到终端
pub fn render_text_to_console(text: &str) {}

