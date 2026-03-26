/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-26
 */
use crate::graphics::{ImageMatrix, TextMatrix};

pub enum PacketType {
    CoverRgb888 = 0x01,
    TextGrayscale = 0x02,
}

fn build_packet(pkt_type: PacketType, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(payload.len() + 8);
    packet.push(0xAA);
    packet.push(0x55);
    packet.push(pkt_type as u8);
    
    let len = payload.len() as u32;
    packet.extend_from_slice(&len.to_le_bytes());
    packet.extend_from_slice(payload);
    
    let checksum = payload.iter().fold(0u8, |acc, &x| acc.wrapping_add(x));
    packet.push(checksum);
    packet
}

// 封装现成的 RGB888 图片矩阵
pub fn pack_cover_matrix(matrix: &ImageMatrix) -> Vec<u8> {
    let mut payload = Vec::with_capacity(matrix.rgb_data.len() + 4);
    payload.extend_from_slice(&(matrix.width as u16).to_le_bytes());
    payload.extend_from_slice(&(matrix.height as u16).to_le_bytes());
    payload.extend_from_slice(&matrix.rgb_data); 
    build_packet(PacketType::CoverRgb888, &payload)
}

// 封装现成的灰度点阵歌词
pub fn pack_text_matrix(matrix: &TextMatrix) -> Vec<u8> {
    let mut payload = Vec::with_capacity(matrix.pixel_data.len() + 4);
    payload.extend_from_slice(&(matrix.width as u16).to_le_bytes());
    payload.extend_from_slice(&(matrix.height as u16).to_le_bytes());
    payload.extend_from_slice(&matrix.pixel_data);
    build_packet(PacketType::TextGrayscale, &payload)
}