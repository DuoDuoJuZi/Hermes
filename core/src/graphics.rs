/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-22
 */
use image::imageops::FilterType;
use rusttype::{Font, PositionedGlyph, Scale, point};
use std::collections::VecDeque;
use std::error::Error;
use std::sync::OnceLock;
use tokio::sync::Mutex;

pub const LYRIC_BITMAP_X: u16 = 360;
pub const LYRIC_BITMAP_Y: u16 = 115;
pub const LYRIC_BITMAP_WIDTH: u16 = 440;
pub const LYRIC_BITMAP_HEIGHT: u16 = 305;
pub const LYRIC_BITMAP_LINES: usize = 11;
#[cfg(test)]
pub const LYRIC_ANIMATION_FRAMES: usize = 5;
const LYRIC_INACTIVE_FONT_SIZE: f32 = 26.0;
const LYRIC_LINE_HEIGHT_FACTOR: f32 = 1.3;
#[cfg(test)]
const LYRIC_SCROLL_DISTANCE: i32 = 42;
const LYRIC_PIXEL_ACTIVE_FLAG: u8 = 0x80;
const LYRIC_PIXEL_LEVEL_MAX: u16 = 0x7F;

#[derive(Clone)]
pub struct LyricBitmap {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

fn encode_lyric_pixel(coverage: u8, is_active: bool) -> u8 {
    if coverage == 0 {
        return 0;
    }

    let level = ((coverage as u16 * LYRIC_PIXEL_LEVEL_MAX + 127) / 255)
        .max(1)
        .min(LYRIC_PIXEL_LEVEL_MAX) as u8;
    if is_active {
        LYRIC_PIXEL_ACTIVE_FLAG | level
    } else {
        level
    }
}

fn decode_lyric_pixel(pixel: u8) -> (bool, u8) {
    if pixel == 0 {
        return (false, 0);
    }

    let is_active = (pixel & LYRIC_PIXEL_ACTIVE_FLAG) != 0;
    let level = if is_active {
        pixel & !LYRIC_PIXEL_ACTIVE_FLAG
    } else {
        pixel
    };
    let coverage =
        ((level as u16 * 255 + LYRIC_PIXEL_LEVEL_MAX / 2) / LYRIC_PIXEL_LEVEL_MAX).min(255) as u8;
    (is_active, coverage)
}

fn put_lyric_pixel(canvas: &mut [u8], idx: usize, pixel: u8) {
    if pixel == 0 {
        return;
    }

    let current = canvas[idx];
    if current == 0 {
        canvas[idx] = pixel;
        return;
    }

    let (current_active, current_coverage) = decode_lyric_pixel(current);
    let (next_active, next_coverage) = decode_lyric_pixel(pixel);
    if next_active && !current_active {
        canvas[idx] = pixel;
    } else if next_active == current_active && next_coverage > current_coverage {
        canvas[idx] = pixel;
    }
}

#[cfg(test)]
fn scale_lyric_pixel(pixel: u8, weight: u16) -> u8 {
    let (is_active, coverage) = decode_lyric_pixel(pixel);
    encode_lyric_pixel(((coverage as u16 * weight) / 255) as u8, is_active)
}

#[cfg(test)]
fn mix_transition_pixels(prev_pixel: u8, prev_weight: u16, next_pixel: u8, next_weight: u16) -> u8 {
    let prev_scaled = scale_lyric_pixel(prev_pixel, prev_weight);
    let next_scaled = scale_lyric_pixel(next_pixel, next_weight);
    let (_, prev_coverage) = decode_lyric_pixel(prev_scaled);
    let (_, next_coverage) = decode_lyric_pixel(next_scaled);

    if next_coverage >= prev_coverage {
        next_scaled
    } else {
        prev_scaled
    }
}

#[cfg(test)]
fn sample_shifted_pixel(bitmap: &LyricBitmap, x: usize, y: usize, y_offset: i32) -> u8 {
    let src_y = y as i32 + y_offset;
    if src_y < 0 || src_y >= bitmap.height as i32 {
        return 0;
    }

    bitmap.pixels[src_y as usize * bitmap.width as usize + x]
}

#[cfg(test)]
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

static FONTS: OnceLock<Vec<Font<'static>>> = OnceLock::new();

fn get_fonts() -> &'static [Font<'static>] {
    FONTS.get_or_init(|| {
        let mut fonts = Vec::new();

        let path_msyh = r#"C:\Windows\Fonts\msyh.ttc"#;
        if let Ok(data) = std::fs::read(path_msyh) {
            if let Some(f) = Font::try_from_vec_and_index(data, 0) {
                fonts.push(f);
            }
        }

        let path_malgun = r#"C:\Windows\Fonts\malgun.ttf"#;
        if let Ok(data) = std::fs::read(path_malgun) {
            if let Some(f) = Font::try_from_vec(data) {
                fonts.push(f);
            }
        }

        let path_msgothic = r#"C:\Windows\Fonts\msgothic.ttc"#;
        if let Ok(data) = std::fs::read(path_msgothic) {
            if let Some(f) = Font::try_from_vec_and_index(data, 0) {
                fonts.push(f);
            }
        }

        fonts
    })
}

/// 图像矩阵数据结构，用于 STM32 等单片机的底层绘制接口
#[derive(Clone)]
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
#[derive(Clone)]
pub struct TextLayer {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub is_active: u8,
    pub pixel_data: Vec<u8>,
    pub line_index: usize,
}

fn normalize_single_lyric_line(line: &str) -> String {
    let line = line.replace(['\r', '\t', '\u{3000}', '\u{00A0}'], " ");
    let has_cjk = line.chars().any(|ch| {
        matches!(
            ch as u32,
            0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        )
    });

    if has_cjk {
        line.chars().filter(|ch| !ch.is_whitespace()).collect()
    } else {
        line.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

fn apply_vertical_edge_feather(
    canvas: &mut [u8],
    top_feather_height: usize,
    bottom_feather_height: usize,
) {
    if top_feather_height == 0 && bottom_feather_height == 0 {
        return;
    }

    let width = LYRIC_BITMAP_WIDTH as usize;
    let height = LYRIC_BITMAP_HEIGHT as usize;
    let top_feather_height = top_feather_height.min(height / 2);
    let bottom_feather_height = bottom_feather_height.min(height / 2);

    for y in 0..height {
        let factor = if top_feather_height > 0 && y < top_feather_height {
            let t = y as f32 / top_feather_height as f32;
            t * t * (3.0 - 2.0 * t)
        } else if bottom_feather_height > 0 && height - 1 - y < bottom_feather_height {
            let t = (height - 1 - y) as f32 / bottom_feather_height as f32;
            t * t * (3.0 - 2.0 * t)
        } else {
            continue;
        };

        for x in 0..width {
            let idx = y * width + x;
            let (is_active, coverage) = decode_lyric_pixel(canvas[idx]);
            canvas[idx] = encode_lyric_pixel((coverage as f32 * factor).round() as u8, is_active);
        }
    }
}

pub fn generate_lyric_bitmap(lines: &[String]) -> LyricBitmap {
    let mut canvas = vec![0u8; LYRIC_BITMAP_WIDTH as usize * LYRIC_BITMAP_HEIGHT as usize];
    let normalized_lines: Vec<String> = lines
        .iter()
        .map(|line| {
            line.lines()
                .map(normalize_single_lyric_line)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();

    if let Some(layers) = generate_text_layers(&normalized_lines) {
        for layer in layers {
            let is_active = layer.is_active == 1;
            for row in 0..layer.height as i32 {
                let dst_y = layer.y as i32 + row - LYRIC_BITMAP_Y as i32;
                if dst_y < 0 || dst_y >= LYRIC_BITMAP_HEIGHT as i32 {
                    continue;
                }

                for col in 0..layer.width as i32 {
                    let dst_x = layer.x as i32 + col - LYRIC_BITMAP_X as i32;
                    if dst_x < 0 || dst_x >= LYRIC_BITMAP_WIDTH as i32 {
                        continue;
                    }

                    let src_idx = row as usize * layer.width as usize + col as usize;
                    let pixel = encode_lyric_pixel(layer.pixel_data[src_idx], is_active);
                    let dst_idx = dst_y as usize * LYRIC_BITMAP_WIDTH as usize + dst_x as usize;
                    put_lyric_pixel(&mut canvas, dst_idx, pixel);
                }
            }
        }
    }

    let inactive_line_h = LYRIC_INACTIVE_FONT_SIZE * LYRIC_LINE_HEIGHT_FACTOR;
    apply_vertical_edge_feather(
        &mut canvas,
        (inactive_line_h / 2.0).round() as usize,
        inactive_line_h.round() as usize,
    );

    LyricBitmap {
        x: LYRIC_BITMAP_X,
        y: LYRIC_BITMAP_Y,
        width: LYRIC_BITMAP_WIDTH,
        height: LYRIC_BITMAP_HEIGHT,
        pixels: canvas,
    }
}

#[cfg(test)]
pub fn generate_lyric_bitmap_transition(
    previous: Option<&LyricBitmap>,
    final_frame: LyricBitmap,
) -> Vec<LyricBitmap> {
    let mut frames = Vec::with_capacity(LYRIC_ANIMATION_FRAMES);

    for step in 1..=LYRIC_ANIMATION_FRAMES {
        let progress = ease_out_cubic(step as f32 / LYRIC_ANIMATION_FRAMES as f32);
        let next_weight = (progress * 255.0).round() as u16;
        let prev_weight = 255 - next_weight;
        let mut frame = final_frame.clone();
        if let Some(previous) = previous {
            let prev_offset = (LYRIC_SCROLL_DISTANCE as f32 * progress).round() as i32;
            let next_offset = (LYRIC_SCROLL_DISTANCE as f32 * (1.0 - progress)).round() as i32;
            frame.pixels.fill(0);

            for y in 0..LYRIC_BITMAP_HEIGHT as usize {
                for x in 0..LYRIC_BITMAP_WIDTH as usize {
                    let prev_pixel = sample_shifted_pixel(previous, x, y, prev_offset);
                    let next_pixel = sample_shifted_pixel(&final_frame, x, y, -next_offset);
                    frame.pixels[y * LYRIC_BITMAP_WIDTH as usize + x] =
                        mix_transition_pixels(prev_pixel, prev_weight, next_pixel, next_weight);
                }
            }
        } else if step != LYRIC_ANIMATION_FRAMES {
            for pixel in &mut frame.pixels {
                *pixel = scale_lyric_pixel(*pixel, next_weight);
            }
        }
        frames.push(frame);
    }

    frames
}

struct CoverCache {
    keys: VecDeque<String>,
    map: std::collections::HashMap<String, ImageMatrix>,
    max_capacity: usize,
}

impl CoverCache {
    fn new(capacity: usize) -> Self {
        Self {
            keys: VecDeque::with_capacity(capacity),
            map: std::collections::HashMap::with_capacity(capacity),
            max_capacity: capacity,
        }
    }

    fn get(&self, key: &str) -> Option<ImageMatrix> {
        self.map.get(key).cloned()
    }

    fn insert(&mut self, key: String, value: ImageMatrix) {
        if self.map.contains_key(&key) {
            return;
        }
        if self.keys.len() >= self.max_capacity {
            if let Some(oldest) = self.keys.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.keys.push_back(key.clone());
        self.map.insert(key, value);
    }
}

static COVER_CACHE: OnceLock<Mutex<CoverCache>> = OnceLock::new();
fn color_luma(r: u8, g: u8, b: u8) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

fn color_saturation(r: u8, g: u8, b: u8) -> f32 {
    let max = r.max(g).max(b) as f32;
    let min = r.min(g).min(b) as f32;
    if max <= 0.0 { 0.0 } else { (max - min) / max }
}

fn darken_theme_color(mut color: (u8, u8, u8)) -> (u8, u8, u8) {
    let luma = color_luma(color.0, color.1, color.2);
    if luma > 80.0 {
        let scale = 80.0 / luma;
        color.0 = (color.0 as f32 * scale).round() as u8;
        color.1 = (color.1 as f32 * scale).round() as u8;
        color.2 = (color.2 as f32 * scale).round() as u8;
    }
    color
}

fn calculate_cover_theme_color(img: &image::RgbImage) -> (u8, u8, u8) {
    let sample = image::imageops::resize(img, 64, 64, FilterType::Triangle);
    let sample = image::imageops::blur(&sample, 1.8);
    let width = sample.width().max(1);
    let height = sample.height().max(1);
    let mut bins = std::collections::HashMap::<u16, (f32, f32, f32, f32, f32)>::new();
    let mut fallback = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);

    for y in 0..height {
        for x in 0..width {
            let pixel = sample.get_pixel(x, y);
            let r = pixel[0];
            let g = pixel[1];
            let b = pixel[2];
            let luma = color_luma(r, g, b);
            let saturation = color_saturation(r, g, b);
            let nx = ((x as f32 + 0.5) / width as f32 - 0.5) * 2.0;
            let ny = ((y as f32 + 0.5) / height as f32 - 0.5) * 2.0;
            let center_weight = (-(nx * nx + ny * ny) * 5.5).exp();

            fallback.0 += r as f32 * center_weight;
            fallback.1 += g as f32 * center_weight;
            fallback.2 += b as f32 * center_weight;
            fallback.3 += center_weight;

            let focus_weight = ((center_weight - 0.25) / 0.75).max(0.0);
            if focus_weight <= 0.0 || !(18.0..=220.0).contains(&luma) || saturation < 0.08 {
                continue;
            }

            let luma_quality = if luma < 95.0 {
                (luma / 95.0).max(0.25)
            } else {
                ((220.0 - luma) / 125.0).max(0.25)
            };
            let score_weight = focus_weight * (0.35 + saturation * 1.8) * luma_quality;
            let avg_weight = focus_weight;
            let key = (((r >> 4) as u16) << 8) | (((g >> 4) as u16) << 4) | ((b >> 4) as u16);
            let entry = bins.entry(key).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 += score_weight;
            entry.1 += r as f32 * avg_weight;
            entry.2 += g as f32 * avg_weight;
            entry.3 += b as f32 * avg_weight;
            entry.4 += avg_weight;
        }
    }

    let color = bins
        .values()
        .filter(|entry| entry.4 > 0.0)
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|entry| {
            (
                (entry.1 / entry.4).round() as u8,
                (entry.2 / entry.4).round() as u8,
                (entry.3 / entry.4).round() as u8,
            )
        })
        .unwrap_or_else(|| {
            if fallback.3 > 0.0 {
                (
                    (fallback.0 / fallback.3).round() as u8,
                    (fallback.1 / fallback.3).round() as u8,
                    (fallback.2 / fallback.3).round() as u8,
                )
            } else {
                (0, 0, 0)
            }
        });

    darken_theme_color(color)
}

/// 异步获取网易云封面并解析降采样为像素矩阵数据
pub async fn fetch_cover_matrix(
    pic_url: &str,
) -> Result<ImageMatrix, Box<dyn Error + Send + Sync>> {
    let cache_mutex = COVER_CACHE.get_or_init(|| Mutex::new(CoverCache::new(50)));
    {
        let cache = cache_mutex.lock().await;
        if let Some(cached) = cache.get(pic_url) {
            return Ok(cached);
        }
    }

    let img_bytes = reqwest::get(pic_url).await?.bytes().await?;
    let img = image::load_from_memory(&img_bytes)?.into_rgb8();
    let orig_w = img.width() as f32;
    let orig_h = img.height() as f32;
    let max_w = 280.0_f32;
    let max_h = 280.0_f32;
    let scale = (max_w / orig_w).min(max_h / orig_h).min(1.0);
    let target_w = (orig_w * scale).max(1.0) as u32;
    let target_h = (orig_h * scale).max(1.0) as u32;
    let resized = image::imageops::resize(&img, target_w, target_h, FilterType::Lanczos3);

    let mut rgb_data = Vec::with_capacity((resized.width() * resized.height() * 3) as usize);
    for y in 0..resized.height() {
        for x in 0..resized.width() {
            let pixel = resized.get_pixel(x, y);
            rgb_data.push(pixel[0]);
            rgb_data.push(pixel[1]);
            rgb_data.push(pixel[2]);
        }
    }

    let theme_color = calculate_cover_theme_color(&img);
    let result = ImageMatrix {
        width: resized.width(),
        height: resized.height(),
        theme_color,
        rgb_data,
    };

    {
        let mut cache = cache_mutex.lock().await;
        cache.insert(pic_url.to_string(), result.clone());
    }

    Ok(result)
}

/// 遍历像素根据 RGB 转义序列直接在控制台彩色打印输出
pub fn print_cover_to_console(_matrix: &ImageMatrix) {
    println!("--- 专辑封面已解析 (控制台预览由于编码问题可能乱码，此处已隐藏) ---");
}

/// 生成文本图层，根据字符串列表渲染带缩放及粗体样式的字体点阵层数据
pub fn generate_text_layers(lines: &[String]) -> Option<Vec<TextLayer>> {
    let fonts = get_fonts();
    if fonts.is_empty() {
        return None;
    }

    let max_width = 410.0;
    let center_idx = lines.len() / 2;

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
        let (size, alpha_mult, is_bold, is_active) = if i == center_idx {
            (40.0, 1.0, true, 1u8)
        } else {
            (26.0, 1.0, true, 0u8)
        };

        let scale = Scale::uniform(size);
        let v_metrics = fonts[0].v_metrics(scale);
        let line_height = size * 1.3;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            blocks.push(LineBlock {
                glyphs: vec![],
                height: 0.0,
                is_active,
            });
            continue;
        }

        let mut block_glyphs = Vec::new();
        let mut current_x = 0.0;
        let mut block_height = line_height;
        let mut current_y = v_metrics.ascent;

        for c in trimmed.chars() {
            if c == '\n' {
                current_x = 0.0;
                current_y += line_height;
                block_height += line_height;
                continue;
            }
            if c == '\r' {
                continue;
            }

            let mut base_glyph = fonts[0].glyph(c);
            for f in fonts.iter().skip(1) {
                if base_glyph.id().0 != 0 {
                    break;
                }
                base_glyph = f.glyph(c);
            }

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
        blocks.push(LineBlock {
            glyphs: block_glyphs,
            height: block_height,
            is_active,
        });
    }

    if blocks.is_empty() {
        return None;
    }

    let n = blocks.len();

    let top_height: f32 = blocks.iter().take(center_idx).map(|b| b.height).sum();
    let bottom_height: f32 = blocks.iter().skip(center_idx + 1).map(|b| b.height).sum();
    let center_height: f32 = blocks[center_idx].height;
    let half_offset = top_height.max(bottom_height);

    let mut y_offsets = vec![0.0_f32; n];
    y_offsets[center_idx] = half_offset;

    let mut y_temp = half_offset;
    if center_idx > 0 {
        for j in (0..center_idx).rev() {
            y_temp -= blocks[j].height;
            y_offsets[j] = y_temp;
        }
    }

    let mut y_temp2 = half_offset + center_height;
    for k in (center_idx + 1)..n {
        y_offsets[k] = y_temp2;
        y_temp2 += blocks[k].height;
    }

    let virtual_total_height = half_offset * 2.0 + center_height;
    let screen_y_base = 120.0 + (360.0 - virtual_total_height) / 2.0 - 30.0;

    let mut layers = Vec::new();

    for (i, block) in blocks.into_iter().enumerate() {
        if block.glyphs.is_empty() {
            continue;
        }

        let mut actual_max_x = 0;
        let mut actual_max_y = 0;
        for g_info in &block.glyphs {
            if let Some(bb) = g_info.glyph.pixel_bounding_box() {
                if bb.max.x > actual_max_x {
                    actual_max_x = bb.max.x;
                }
                if bb.max.y > actual_max_y {
                    actual_max_y = bb.max.y;
                }
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
                        if px >= 0 && px < actual_width as i32 && py >= 0 && py < height_ceil as i32
                        {
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
        let start_x = 360;

        layers.push(TextLayer {
            x: start_x as i16,
            y: y_float as i16,
            width: actual_width as u16,
            height: height_ceil as u16,
            is_active: block.is_active,
            pixel_data,
            line_index: i,
        });
    }

    Some(layers)
}

/// 完全独立渲染歌曲标题与歌手专辑元数据信息
pub fn generate_meta_layers(title: &str, subtitle: &str) -> Option<Vec<TextLayer>> {
    let fonts = get_fonts();
    if fonts.is_empty() {
        return None;
    }

    let max_width = 410.0;
    let start_x = 360.0;
    let mut current_y = 10.0;
    let mut layers = Vec::new();

    let meta_lines = vec![(title, 46.0, true, 1u8), (subtitle, 22.0, false, 0u8)];
    for (line, size, is_bold, is_active) in meta_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let scale = Scale::uniform(size);
        let v_metrics = fonts[0].v_metrics(scale);
        let line_height = size * 1.3;

        let mut actual_max_x = 0;
        let mut actual_max_y = 0;
        let mut glyphs = Vec::new();
        let mut current_x = 0.0;
        let block_y_asc = v_metrics.ascent;

        for c in trimmed.chars() {
            let mut base_glyph = fonts[0].glyph(c);
            for f in fonts.iter().skip(1) {
                if base_glyph.id().0 != 0 {
                    break;
                }
                base_glyph = f.glyph(c);
            }

            let scaled_glyph = base_glyph.scaled(scale);
            let h_metrics = scaled_glyph.h_metrics();

            if current_x + h_metrics.advance_width > max_width && current_x > 0.0 {
                break;
            }

            let positioned = scaled_glyph.positioned(point(current_x, block_y_asc));
            current_x += h_metrics.advance_width;

            let bb_opt = positioned.pixel_bounding_box();
            if let Some(bb) = bb_opt {
                if bb.max.x > actual_max_x {
                    actual_max_x = bb.max.x;
                }
                if bb.max.y > actual_max_y {
                    actual_max_y = bb.max.y;
                }
            }
            glyphs.push((positioned, bb_opt));
        }

        let actual_width = (actual_max_x as usize).max(1);
        let height_ceil = (actual_max_y as usize).max(1);

        let mut pixel_data = vec![0u8; actual_width * height_ceil];
        for (glyph, bb_opt) in glyphs {
            if let Some(bb) = bb_opt {
                glyph.draw(|x, y, v| {
                    let mut draw_pixel = |dx: i32| {
                        let px = bb.min.x + x as i32 + dx;
                        let py = bb.min.y + y as i32;
                        if px >= 0 && px < actual_width as i32 && py >= 0 && py < height_ceil as i32
                        {
                            let idx = (py as usize) * actual_width + (px as usize);
                            let val = (v * 255.0) as u8;
                            if val > pixel_data[idx] {
                                pixel_data[idx] = val;
                            }
                        }
                    };
                    draw_pixel(0);
                    if is_bold {
                        draw_pixel(1);
                    }
                });
            }
        }

        layers.push(TextLayer {
            x: start_x as i16,
            y: current_y as i16,
            width: actual_width as u16,
            height: height_ceil as u16,
            is_active,
            pixel_data,
            line_index: 0,
        });

        current_y += line_height + 5.0;
    }

    Some(layers)
}

/// 渲染时间文本（当前时间 / 总时间）
pub fn generate_time_layer(time_str: &str, x: i16, y: i16) -> Option<TextLayer> {
    let fonts = get_fonts();
    if fonts.is_empty() {
        return None;
    }

    let size = 16.0;
    let scale = Scale::uniform(size);
    let v_metrics = fonts[0].v_metrics(scale);
    let trimmed = time_str.trim();

    let mut actual_max_x = 0;
    let mut actual_max_y = 0;
    let mut glyphs = Vec::new();
    let mut current_x = 0.0;
    let block_y_asc = v_metrics.ascent;

    for c in trimmed.chars() {
        let mut base_glyph = fonts[0].glyph(c);
        for f in fonts.iter().skip(1) {
            if base_glyph.id().0 != 0 {
                break;
            }
            base_glyph = f.glyph(c);
        }

        let scaled_glyph = base_glyph.scaled(scale);
        let h_metrics = scaled_glyph.h_metrics();

        let positioned = scaled_glyph.positioned(point(current_x, block_y_asc));
        current_x += h_metrics.advance_width;

        let bb_opt = positioned.pixel_bounding_box();
        if let Some(bb) = bb_opt {
            if bb.max.x > actual_max_x {
                actual_max_x = bb.max.x;
            }
            if bb.max.y > actual_max_y {
                actual_max_y = bb.max.y;
            }
        }
        glyphs.push((positioned, bb_opt));
    }

    let actual_width = (actual_max_x as usize).max(1);
    let height_ceil = (actual_max_y as usize).max(1);

    let mut pixel_data = vec![0u8; actual_width * height_ceil];
    for (glyph, bb_opt) in glyphs {
        if let Some(bb) = bb_opt {
            glyph.draw(|gx, gy, v| {
                let px = bb.min.x + gx as i32;
                let py = bb.min.y + gy as i32;
                if px >= 0 && px < actual_width as i32 && py >= 0 && py < height_ceil as i32 {
                    let idx = (py as usize) * actual_width + (px as usize);
                    let val = (v * 255.0) as u8;
                    if val > pixel_data[idx] {
                        pixel_data[idx] = val;
                    }
                }
            });
        }
    }

    Some(TextLayer {
        x,
        y,
        width: actual_width as u16,
        height: height_ceil as u16,
        is_active: 0,
        pixel_data,
        line_index: 0,
    })
}

/// 打印文本像素点阵到控制台
pub fn print_text_matrix(matrix: &TextMatrix, text: &str) {}

/// 生成媒体控制图层（上一曲、播放/暂停、下一曲）
pub fn generate_media_controls_layers(is_play: bool) -> Vec<TextLayer> {
    let width: usize = 40;
    let height: usize = 40;

    let mut play_data = vec![0u8; width * height];
    if is_play {
        for y in 6..34 {
            for x in 8..16 {
                play_data[y * width + x] = 255;
            }
            for x in 24..32 {
                play_data[y * width + x] = 255;
            }
        }
    } else {
        for y in 6..34 {
            let half = (y as f32 - 20.0).abs();
            let limit = 32.0 - half * (22.0 / 14.0);
            for x in 10..34 {
                if (x as f32) < limit {
                    play_data[y * width + x] = 255;
                }
            }
        }
    }

    let mut prev_data = vec![0u8; width * height];
    for y in 10..30 {
        for x in 6..10 {
            prev_data[y * width + x] = 255;
        }
    }
    for y in 10..30 {
        let half = (y as f32 - 20.0).abs();
        let left_edge = 12.0 + half * (20.0 / 10.0);
        for x in 10..34 {
            if (x as f32) > left_edge {
                prev_data[y * width + x] = 255;
            }
        }
    }

    let mut next_data = vec![0u8; width * height];
    for y in 10..30 {
        for x in 30..34 {
            next_data[y * width + x] = 255;
        }
    }
    for y in 10..30 {
        let half = (y as f32 - 20.0).abs();
        let right_edge = 28.0 - half * (20.0 / 10.0);
        for x in 6..30 {
            if (x as f32) < right_edge {
                next_data[y * width + x] = 255;
            }
        }
    }

    vec![
        TextLayer {
            x: 160 - 20,
            y: 380,
            width: width as u16,
            height: height as u16,
            is_active: 1,
            pixel_data: play_data,
            line_index: 0,
        },
        TextLayer {
            x: 80 - 20,
            y: 380,
            width: width as u16,
            height: height as u16,
            is_active: 1,
            pixel_data: prev_data,
            line_index: 0,
        },
        TextLayer {
            x: 240 - 20,
            y: 380,
            width: width as u16,
            height: height as u16,
            is_active: 1,
            pixel_data: next_data,
            line_index: 0,
        },
    ]
}

/// 直接将字符串渲染成像素矩阵并输出到终端
pub fn render_text_to_console(text: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyric_bitmap_has_fixed_canvas_size() {
        let lines = vec![
            "short".to_string(),
            "这是一句很长很长的中文歌词，用来确认第一版固定画布不会因为文本长度改变尺寸"
                .to_string(),
            "日本語の歌詞もここに入ります".to_string(),
            "한국어 가사도 들어갑니다".to_string(),
            "line 4".to_string(),
            "active center lyric".to_string(),
            "line 6".to_string(),
            "line 7".to_string(),
            "line 8".to_string(),
            "line 9".to_string(),
            "line 10".to_string(),
        ];
        let bitmap = generate_lyric_bitmap(&lines);
        assert_eq!(bitmap.width, LYRIC_BITMAP_WIDTH);
        assert_eq!(bitmap.height, LYRIC_BITMAP_HEIGHT);
        assert_eq!(
            bitmap.pixels.len(),
            LYRIC_BITMAP_WIDTH as usize * LYRIC_BITMAP_HEIGHT as usize
        );
    }

    #[test]
    fn lyric_bitmap_accepts_overscan_lines() {
        let lines: Vec<String> = (0..LYRIC_BITMAP_LINES + 2)
            .map(|i| format!("overscan lyric line {i}"))
            .collect();
        let bitmap = generate_lyric_bitmap(&lines);

        assert_eq!(bitmap.width, LYRIC_BITMAP_WIDTH);
        assert_eq!(bitmap.height, LYRIC_BITMAP_HEIGHT);
        assert!(
            bitmap
                .pixels
                .iter()
                .any(|&pixel| (pixel & LYRIC_PIXEL_ACTIVE_FLAG) != 0)
        );
        assert!(
            bitmap
                .pixels
                .iter()
                .any(|&pixel| pixel > 0 && pixel < LYRIC_PIXEL_ACTIVE_FLAG)
        );
    }

    #[test]
    fn lyric_bitmap_feathers_edge_lines() {
        let lines: Vec<String> = (0..LYRIC_BITMAP_LINES + 2)
            .map(|i| format!("lyric line {i}"))
            .collect();
        let bitmap = generate_lyric_bitmap(&lines);
        let width = LYRIC_BITMAP_WIDTH as usize;

        assert!(bitmap.pixels[..width].iter().all(|&pixel| pixel == 0));
        assert!(bitmap.pixels[..width * 64].iter().any(|&pixel| pixel > 0));
        assert!(
            bitmap.pixels[width * (LYRIC_BITMAP_HEIGHT as usize - 1)..]
                .iter()
                .all(|&pixel| pixel == 0)
        );
        assert!(
            bitmap.pixels[width * (LYRIC_BITMAP_HEIGHT as usize - 64)..]
                .iter()
                .any(|&pixel| pixel > 0)
        );
    }

    #[test]
    fn lyric_bitmap_preserves_active_and_inactive_color_tags() {
        let lines: Vec<String> = (0..LYRIC_BITMAP_LINES)
            .map(|i| format!("lyric line {i}"))
            .collect();
        let bitmap = generate_lyric_bitmap(&lines);

        assert!(
            bitmap
                .pixels
                .iter()
                .any(|&pixel| (pixel & LYRIC_PIXEL_ACTIVE_FLAG) != 0)
        );
        assert!(
            bitmap
                .pixels
                .iter()
                .any(|&pixel| pixel > 0 && pixel < LYRIC_PIXEL_ACTIVE_FLAG)
        );
    }

    #[test]
    fn lyric_transition_preserves_color_tags() {
        let previous = generate_lyric_bitmap(
            &(0..LYRIC_BITMAP_LINES)
                .map(|i| format!("previous lyric line {i}"))
                .collect::<Vec<_>>(),
        );
        let final_frame = generate_lyric_bitmap(
            &(0..LYRIC_BITMAP_LINES)
                .map(|i| format!("next lyric line {i}"))
                .collect::<Vec<_>>(),
        );

        let frames = generate_lyric_bitmap_transition(Some(&previous), final_frame.clone());

        assert!(frames.iter().all(|frame| {
            frame
                .pixels
                .iter()
                .any(|&pixel| (pixel & LYRIC_PIXEL_ACTIVE_FLAG) != 0)
        }));
        assert!(frames.iter().all(|frame| {
            frame
                .pixels
                .iter()
                .any(|&pixel| pixel > 0 && pixel < LYRIC_PIXEL_ACTIVE_FLAG)
        }));
        assert_ne!(
            frames.first().map(|frame| &frame.pixels),
            Some(&final_frame.pixels)
        );
        assert_eq!(
            frames.last().map(|frame| &frame.pixels),
            Some(&final_frame.pixels)
        );
    }

    #[test]
    fn lyric_normalization_removes_cjk_internal_spaces_but_keeps_latin_word_spaces() {
        assert_eq!(normalize_single_lyric_line("你  好　世\t界"), "你好世界");
        assert_eq!(
            normalize_single_lyric_line("hello   beautiful\tworld"),
            "hello beautiful world"
        );
    }
    #[test]
    fn cover_theme_prefers_center_subject_over_large_edge_color() {
        let mut img = image::RgbImage::from_pixel(96, 96, image::Rgb([30, 80, 210]));
        for y in 34..62 {
            for x in 34..62 {
                img.put_pixel(x, y, image::Rgb([220, 30, 30]));
            }
        }

        let theme = calculate_cover_theme_color(&img);

        assert!(theme.0 > theme.2, "theme should lean red, got {theme:?}");
        assert!(color_luma(theme.0, theme.1, theme.2) <= 81.0);
    }
}
