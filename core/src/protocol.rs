/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-26
 */
use crate::graphics::{ImageMatrix, TextMatrix, TextLayer};

/// 定义与下位机通信的不同逻辑功能类型
pub enum PacketType {
    CoverRgb888 = 0x01,
    TextGrayscale = 0x02,
    ClearRect = 0x03,
    TextLayer = 0x04,
    Progress = 0x05,
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