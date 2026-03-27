/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-27
 * @brief 串口数据协议解析源代码，负责解析接收到的指令并驱动 LCD 进行绘制。
 */
#include "protocol_parser.h"
#include "lcd_rgb.h"

typedef enum {
    STATE_HEAD1 = 0,
    STATE_HEAD2,
    STATE_TYPE,
    STATE_LEN,
    STATE_PAYLOAD,
    STATE_CHECKSUM
} ParserState;

typedef struct {
    ParserState state;
    uint8_t type;
    uint32_t len;
    uint32_t len_cnt;
    uint32_t payload_cnt;
    uint8_t checksum_calc;
    uint8_t *payload_buf;
} ProtocolParser;

ProtocolParser parser;

uint8_t rx_payload_buffer[82000];

/**
 * @brief 绘制全彩封面图像
 * @param data 图像像素及宽高数据
 * @param length 数据总长度
 * @return 无
 */
void Draw_Cover(uint8_t *data, uint32_t length) {
    uint16_t width = data[0] | (data[1] << 8);
    uint16_t height = data[2] | (data[3] << 8);
    uint32_t data_idx = 4;
    int16_t start_x = (400 - (int16_t)width) / 2;
    if (start_x < 0) start_x = 0;
    int16_t start_y = (480 - (int16_t)height) / 2;
    if (start_y < 0) start_y = 0;

    LCD_SetColor(0xFF000000);
    LCD_FillRect(0, 0, 400, 480);

    for (uint16_t y = 0; y < height; y++) {
        for (uint16_t x = 0; x < width; x++) {
            if (data_idx + 2 < length) {
                uint8_t r = data[data_idx++];
                uint8_t g = data[data_idx++];
                uint8_t b = data[data_idx++];
                uint32_t color = (0xFF << 24) | (r << 16) | (g << 8) | b;

                if (start_x + x < 400 && start_y + y < 480) {
                    LCD_DrawPoint(start_x + x, start_y + y, color);
                }
            }
        }
    }
}

/**
 * @brief 绘制灰度歌词文本
 * @param data 文本点阵灰度像素及宽高数据
 * @param length 数据总长度
 * @return 无
 */
void Draw_TextGrayscale(uint8_t *data, uint32_t length) {
    uint16_t width = data[0] | (data[1] << 8);
    uint16_t height = data[2] | (data[3] << 8);
    uint32_t data_idx = 4;
    
    // 右侧显示歌词，支持在右侧(x:400~800)内显示
    int16_t start_x = 400 + (400 - (int16_t)width) / 2;
    if (start_x < 410) start_x = 410; // 保留一定左边距避免紧贴中线
    int16_t start_y = (480 - (int16_t)height) / 2;
    if (start_y < 0) start_y = 0;
    uint32_t bg_color = 0xFF000000;

    // 清空整个右侧或者歌词区域
    LCD_SetColor(bg_color);
    LCD_FillRect(400, 0, 400, 480);

    for (uint16_t y = 0; y < height; y++) {
        for (uint16_t x = 0; x < width; x++) {
            if (data_idx < length) {
                uint8_t alpha = data[data_idx++];
                if (alpha > 10) {
                    if (start_x + x >= 400 && start_x + x < 800 && start_y + y < 480) {
                        uint32_t text_color = (0xFF << 24) | (alpha << 16) | (alpha << 8) | alpha;
                        LCD_DrawPoint(start_x + x, start_y + y, text_color);
                    }
                }
            }
        }
    }
}/**
 * @brief 初始化协议解析状态机
 * @param 无
 * @return 无
 */
void Protocol_Init(void) {
    parser.state = STATE_HEAD1;
    parser.payload_buf = rx_payload_buffer;
}

/**
 * @brief 解析单个字节并驱动状态机运转
 * @param byte 接收到的单个字节数据
 * @return 无
 */
void Protocol_ParseByte(uint8_t byte) {
    switch (parser.state) {
        case STATE_HEAD1:
            if (byte == 0xAA) parser.state = STATE_HEAD2;
            break;
            
        case STATE_HEAD2:
            if (byte == 0x55) parser.state = STATE_TYPE;
            else parser.state = STATE_HEAD1;
            break;
            
        case STATE_TYPE:
            parser.type = byte;
            parser.len = 0;
            parser.len_cnt = 0;
            parser.state = STATE_LEN;
            break;
            
        case STATE_LEN:
            parser.len |= (byte << (8 * parser.len_cnt));
            parser.len_cnt++;
            if (parser.len_cnt == 4) {
                parser.payload_cnt = 0;
                parser.checksum_calc = 0;
                if (parser.len > 0 && parser.len <= sizeof(rx_payload_buffer)) {
                    parser.state = STATE_PAYLOAD;
                } else {
                    parser.state = STATE_HEAD1;
                }
            }
            break;
            
        case STATE_PAYLOAD:
            parser.payload_buf[parser.payload_cnt++] = byte;
            parser.checksum_calc += byte;
            if (parser.payload_cnt >= parser.len) {
                parser.state = STATE_CHECKSUM;
            }
            break;
            
        case STATE_CHECKSUM:
            if (byte == parser.checksum_calc) {
                if (parser.type == 0x01) {
                    Draw_Cover(parser.payload_buf, parser.len);
                } else if (parser.type == 0x02) {
                    Draw_TextGrayscale(parser.payload_buf, parser.len);
                } else if (parser.type == 0x03) {
                    // TYPE 0x03: 清除指定区域
                    uint16_t x = parser.payload_buf[0] | (parser.payload_buf[1] << 8);
                    uint16_t y = parser.payload_buf[2] | (parser.payload_buf[3] << 8);
                    uint16_t w = parser.payload_buf[4] | (parser.payload_buf[5] << 8);
                    uint16_t h = parser.payload_buf[6] | (parser.payload_buf[7] << 8);
                    LCD_SetColor(0xFF000000);
                    LCD_FillRect(x, y, w, h);
                } else if (parser.type == 0x04) {
                    // TYPE 0x04: 在指定坐标绘制小块灰度图
                    uint16_t x_off = parser.payload_buf[0] | (parser.payload_buf[1] << 8);
                    uint16_t y_off = parser.payload_buf[2] | (parser.payload_buf[3] << 8);
                    uint16_t w = parser.payload_buf[4] | (parser.payload_buf[5] << 8);
                    uint16_t h = parser.payload_buf[6] | (parser.payload_buf[7] << 8);
                    uint32_t data_idx = 8;
                    for (uint16_t y = 0; y < h; y++) {
                        for (uint16_t x = 0; x < w; x++) {
                            if (data_idx < parser.len) {
                                uint8_t alpha = parser.payload_buf[data_idx++];
                                if (alpha > 10) {
                                    if (x_off + x < 800 && y_off + y < 480) {
                                        uint32_t text_color = (0xFF << 24) | (alpha << 16) | (alpha << 8) | alpha;
                                        LCD_DrawPoint(x_off + x, y_off + y, text_color);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            parser.state = STATE_HEAD1;
            break;
            
        default:
            parser.state = STATE_HEAD1;
            break;
    }
}