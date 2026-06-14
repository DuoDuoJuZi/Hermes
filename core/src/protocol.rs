/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-26
 */
use crate::graphics::{ImageMatrix, LyricBitmap, TextLayer, TextMatrix};

/// 定义与下位机通信的不同逻辑功能类型
pub enum PacketType {
    CoverRgb888 = 0x01,
    TextGrayscale = 0x02,
    ClearRect = 0x03,
    TextLayer = 0x04,
    Progress = 0x05,
    LyricBitmap = 0x06,
}

/// 构建底层原始包裹结构并计算校验和
fn build_packet_raw(pkt_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 8);
    packet.push(0xAA);
    packet.push(0x55);
    packet.push(pkt_type);

    let len = payload.len() as u32;
    packet.extend_from_slice(&len.to_le_bytes());
    packet.extend_from_slice(payload);

    let checksum = payload.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
    packet.push(checksum);
    packet
}

/// 接收上层包类型枚举，封装具体协议类型
fn build_packet(pkt_type: PacketType, payload: &[u8]) -> Vec<u8> {
    build_packet_raw(pkt_type as u8, payload)
}

/// 封装清除显示屏上的指定区域矩形的指令包
pub fn pack_clear_rect(x: u16, y: u16, width: u16, height: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&x.to_le_bytes());
    payload.extend_from_slice(&y.to_le_bytes());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    build_packet(PacketType::ClearRect, &payload)
}

/// 封装包含小块灰度文本图层的指令包
pub fn pack_text_layer(layer: &TextLayer) -> Vec<u8> {
    let mut payload = Vec::with_capacity(layer.pixel_data.len() + 9);
    payload.extend_from_slice(&layer.x.to_le_bytes());
    payload.extend_from_slice(&layer.y.to_le_bytes());
    payload.extend_from_slice(&layer.width.to_le_bytes());
    payload.extend_from_slice(&layer.height.to_le_bytes());
    payload.push(layer.is_active);
    payload.extend_from_slice(&layer.pixel_data);
    build_packet(PacketType::TextLayer, &payload)
}

pub fn encode_rle_grayscale(pixels: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::new();
    if pixels.is_empty() {
        return encoded;
    }

    let mut run_value = pixels[0];
    let mut run_len: u16 = 1;

    for &pixel in pixels.iter().skip(1) {
        if pixel == run_value && run_len < u16::MAX {
            run_len += 1;
        } else {
            encoded.extend_from_slice(&run_len.to_le_bytes());
            encoded.push(run_value);
            run_value = pixel;
            run_len = 1;
        }
    }

    encoded.extend_from_slice(&run_len.to_le_bytes());
    encoded.push(run_value);
    encoded
}

pub fn pack_lyric_bitmap(bitmap: &LyricBitmap) -> Vec<u8> {
    let rle = encode_rle_grayscale(&bitmap.pixels);
    let use_rle = rle.len() < bitmap.pixels.len();
    let data = if use_rle {
        rle.as_slice()
    } else {
        bitmap.pixels.as_slice()
    };

    let mut payload = Vec::with_capacity(data.len() + 9);
    payload.extend_from_slice(&bitmap.x.to_le_bytes());
    payload.extend_from_slice(&bitmap.y.to_le_bytes());
    payload.extend_from_slice(&bitmap.width.to_le_bytes());
    payload.extend_from_slice(&bitmap.height.to_le_bytes());
    payload.push(if use_rle { 1 } else { 0 });
    payload.extend_from_slice(data);
    build_packet(PacketType::LyricBitmap, &payload)
}

pub fn pack_lyric_bitmap_cropped(bitmap: &LyricBitmap) -> Vec<u8> {
    let width = bitmap.width as usize;
    let height = bitmap.height as usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;

    for y in 0..height {
        for x in 0..width {
            if bitmap.pixels[y * width + x] == 0 {
                continue;
            }
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if min_x == width {
        return pack_lyric_bitmap(&LyricBitmap {
            x: bitmap.x,
            y: bitmap.y,
            width: 1,
            height: 1,
            pixels: vec![0],
        });
    }

    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    let mut pixels = Vec::with_capacity(crop_w * crop_h);
    for y in min_y..=max_y {
        let row_start = y * width + min_x;
        pixels.extend_from_slice(&bitmap.pixels[row_start..row_start + crop_w]);
    }

    let cropped = LyricBitmap {
        x: bitmap.x + min_x as u16,
        y: bitmap.y + min_y as u16,
        width: crop_w as u16,
        height: crop_h as u16,
        pixels,
    };

    let full_packet = pack_lyric_bitmap(bitmap);
    let cropped_packet = pack_lyric_bitmap(&cropped);
    if cropped_packet.len() < full_packet.len() {
        cropped_packet
    } else {
        full_packet
    }
}

/// 封装现成的 RGB888 图像矩阵
pub fn pack_cover_matrix(matrix: &ImageMatrix) -> Vec<u8> {
    let mut payload = Vec::with_capacity(matrix.rgb_data.len() + 7);
    payload.extend_from_slice(&(matrix.width as u16).to_le_bytes());
    payload.extend_from_slice(&(matrix.height as u16).to_le_bytes());
    payload.push(matrix.theme_color.0);
    payload.push(matrix.theme_color.1);
    payload.push(matrix.theme_color.2);
    payload.extend_from_slice(&matrix.rgb_data);
    build_packet(PacketType::CoverRgb888, &payload)
}

/// 封装现成的灰度点阵歌词
pub fn pack_text_matrix(matrix: &TextMatrix) -> Vec<u8> {
    let mut payload = Vec::with_capacity(matrix.pixel_data.len() + 4);
    payload.extend_from_slice(&(matrix.width as u16).to_le_bytes());
    payload.extend_from_slice(&(matrix.height as u16).to_le_bytes());
    payload.extend_from_slice(&matrix.pixel_data);
    build_packet(PacketType::TextGrayscale, &payload)
}

/// 封装播放进度包，千分比
pub fn pack_progress(permille: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2);
    payload.extend_from_slice(&permille.to_le_bytes());
    build_packet(PacketType::Progress, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_rle(encoded: &[u8]) -> Vec<u8> {
        let mut decoded = Vec::new();
        for chunk in encoded.chunks_exact(3) {
            let len = u16::from_le_bytes([chunk[0], chunk[1]]) as usize;
            decoded.extend(std::iter::repeat_n(chunk[2], len));
        }
        decoded
    }

    fn unpack_lyric_bitmap_pixels(packet: &[u8]) -> Vec<u8> {
        assert_eq!(packet[0], 0xAA);
        assert_eq!(packet[1], 0x55);
        assert_eq!(packet[2], PacketType::LyricBitmap as u8);
        let len = u32::from_le_bytes([packet[3], packet[4], packet[5], packet[6]]) as usize;
        let payload = &packet[7..7 + len];
        let encoding = payload[8];
        let data = &payload[9..];

        if encoding == 1 {
            decode_rle(data)
        } else {
            data.to_vec()
        }
    }

    #[test]
    fn rle_encodes_empty_canvas() {
        assert!(encode_rle_grayscale(&[]).is_empty());
    }

    #[test]
    fn rle_round_trips_repeated_pixels() {
        let pixels = vec![0; 1000];
        let encoded = encode_rle_grayscale(&pixels);
        assert!(encoded.len() < pixels.len());
        assert_eq!(decode_rle(&encoded), pixels);
    }

    #[test]
    fn rle_round_trips_mixed_pixels() {
        let pixels: Vec<u8> = (0..2048).map(|i| ((i * 37 + 11) % 251) as u8).collect();
        assert_eq!(decode_rle(&encode_rle_grayscale(&pixels)), pixels);
    }

    #[test]
    fn lyric_bitmap_packet_preserves_tagged_pixels() {
        let bitmap = LyricBitmap {
            x: 360,
            y: 115,
            width: 4,
            height: 2,
            pixels: vec![0, 1, 0x7F, 0x80, 0x81, 0xFF, 0, 0x42],
        };

        let packet = pack_lyric_bitmap(&bitmap);

        assert_eq!(unpack_lyric_bitmap_pixels(&packet), bitmap.pixels);
    }

    #[test]
    fn lyric_bitmap_cropped_packet_reduces_empty_edges() {
        let bitmap = LyricBitmap {
            x: 360,
            y: 115,
            width: 8,
            height: 6,
            pixels: vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 5, 0, 0, 0, 0, 0, 0, 6, 7,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        };

        let packet = pack_lyric_bitmap_cropped(&bitmap);
        let len = u32::from_le_bytes([packet[3], packet[4], packet[5], packet[6]]) as usize;
        let payload = &packet[7..7 + len];

        assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), 362);
        assert_eq!(u16::from_le_bytes([payload[2], payload[3]]), 117);
        assert_eq!(u16::from_le_bytes([payload[4], payload[5]]), 2);
        assert_eq!(u16::from_le_bytes([payload[6], payload[7]]), 2);
        assert_eq!(unpack_lyric_bitmap_pixels(&packet), vec![4, 5, 6, 7]);
    }
}
