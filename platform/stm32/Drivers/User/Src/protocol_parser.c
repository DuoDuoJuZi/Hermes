/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-27
 * @brief 串口数据协议解析源代码，负责解析接收到的指令并驱动 LCD 进行绘制
 */
#include "protocol_parser.h"
#include "lcd_rgb.h"

/**
 * @brief 安全绘制带有正确颜色模式转换的点用于封面图层
 * @param x 水平坐标
 * @param y 垂直坐标
 * @param r 红色分量
 * @param g 绿色分量
 * @param b 蓝色分量
 */
static void Safe_DrawPoint_Layer0(uint16_t x, uint16_t y, uint8_t r, uint8_t g, uint8_t b) {
    uint32_t final_color = 0;
#if ColorMode_0 == LTDC_PIXEL_FORMAT_RGB565
    final_color = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
#elif ColorMode_0 == LTDC_PIXEL_FORMAT_ARGB1555
    final_color = 0x8000 | ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
#else
    final_color = (0xFF << 24) | (r << 16) | (g << 8) | b;
#endif
    LCD_DrawPoint(x, y, final_color);
}

/**
 * @brief 安全绘制带有正确颜色模式转换的点用于歌词图层
 * @param x 水平坐标
 * @param y 垂直坐标
 * @param r 红色分量
 * @param g 绿色分量
 * @param b 蓝色分量
 */
static void Safe_DrawPoint_Layer1(uint16_t x, uint16_t y, uint8_t r, uint8_t g, uint8_t b) {
    uint32_t final_color = 0;
#if LCD_NUM_LAYERS == 2
    #if ColorMode_1 == LTDC_PIXEL_FORMAT_RGB565
        final_color = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
    #elif ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB1555
        final_color = 0x8000 | ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
    #else
        final_color = (0xFF << 24) | (r << 16) | (g << 8) | b;
    #endif
#else
    final_color = (0xFF << 24) | (r << 16) | (g << 8) | b;
#endif
    LCD_DrawPoint(x, y, final_color);
}

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

#define RX_PAYLOAD_BUF_SIZE 250000
uint8_t *const rx_payload_buffer = (uint8_t *)0x24000000;

static uint32_t global_theme_bg = 0xFF000000;


/**
 * @brief 绘制全彩封面图像
 * @param data 图像像素及宽高数据
 * @param length 数据总长度
 */
void Draw_Cover(uint8_t *data, uint32_t length) {
    uint16_t width = data[0] | (data[1] << 8);
    uint16_t height = data[2] | (data[3] << 8);
    uint8_t theme_r = data[4];
    uint8_t theme_g = data[5];
    uint8_t theme_b = data[6];
    uint32_t data_idx = 7;
    int16_t start_x = 20 + (300 - 20 - (int16_t)width) / 2;
    if (start_x < 20) start_x = 20;
    int16_t start_y = (480 - (int16_t)height) / 2 - 30;
    if (start_y < 0) start_y = 0;

    global_theme_bg = (0xFF << 24) | (theme_r << 16) | (theme_g << 8) | theme_b;

    LCD_SetLayer(1);
    LCD_SetColor(0x00000000);
    LCD_FillRect(0, 0, 800, 380);
    LCD_FillRect(300, 380, 500, 60);

    LCD_SetLayer(0);
    LCD_SetColor(global_theme_bg);
    LCD_FillRect(0, 0, 800, 480);

    for (uint16_t y = 0; y < height; y++) {
        for (uint16_t x = 0; x < width; x++) {
            if (data_idx + 2 < length) {
                uint8_t r = data[data_idx++];
                uint8_t g = data[data_idx++];
                uint8_t b = data[data_idx++];

                if (start_x + x >= 20 && start_x + x < 300 && start_y + y >= 50 && start_y + y < 450) {
                    Safe_DrawPoint_Layer0(start_x + x, start_y + y, r, g, b);
                }
            }
        }
    }
}

/**
 * @brief 绘制灰度歌词文本
 * @param data 文本点阵灰度像素及宽高数据
 * @param length 数据总长度
 */
void Draw_TextGrayscale(uint8_t *data, uint32_t length) {
    uint16_t width = data[0] | (data[1] << 8);
    uint16_t height = data[2] | (data[3] << 8);
    uint32_t data_idx = 4;

    int16_t start_x = 300 + (500 - (int16_t)width) / 2;
    if (start_x < 310) start_x = 310;
    int16_t start_y = (440 - (int16_t)height) / 2;
    if (start_y < 0) start_y = 0;

    LCD_SetLayer(1);
    LCD_SetColor(0x00000000);
    LCD_FillRect(300, 0, 500, 440);

    for (uint16_t y = 0; y < height; y++) {
        for (uint16_t x = 0; x < width; x++) {
            if (data_idx < length) {
                uint8_t alpha = data[data_idx++];
                if (alpha > 10) {
                    if (start_x + x >= 300 && start_x + x < 800 && start_y + y < 440) {
                        Safe_DrawPoint_Layer1(start_x + x, start_y + y, alpha, alpha, alpha);
                    }
                }
            }
        }
    }
}

/**
 * @brief 初始化协议解析状态机
 */
void Protocol_Init(void) {
    parser.state = STATE_HEAD1;
    parser.payload_buf = rx_payload_buffer;
    
    LCD_SetLayer(1);
    LCD_SetColor(0x00000000);
    LCD_FillRect(0, 0, 800, 480);
    
    LCD_SetLayer(0);
    LCD_SetColor(global_theme_bg);
    LCD_FillRect(0, 0, 800, 480);
}

/**
 * @brief 解析单个字节并驱动状态机运转
 * @param byte 接收到的单个字节数据
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
                if (parser.len > 0 && parser.len <= RX_PAYLOAD_BUF_SIZE) {
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
                    uint16_t x = parser.payload_buf[0] | (parser.payload_buf[1] << 8);
                    uint16_t y = parser.payload_buf[2] | (parser.payload_buf[3] << 8);
                    uint16_t w = parser.payload_buf[4] | (parser.payload_buf[5] << 8);
                    uint16_t h = parser.payload_buf[6] | (parser.payload_buf[7] << 8);
                    LCD_SetLayer(1);
                    LCD_SetColor(0x00000000);
                    LCD_FillRect(x, y, w, h);
                } else if (parser.type == 0x04) {
                    int16_t x_off = (int16_t)(parser.payload_buf[0] | (parser.payload_buf[1] << 8));
                    int16_t y_off = (int16_t)(parser.payload_buf[2] | (parser.payload_buf[3] << 8));
                    uint16_t w = parser.payload_buf[4] | (parser.payload_buf[5] << 8);
                    uint16_t h = parser.payload_buf[6] | (parser.payload_buf[7] << 8);
                    uint8_t is_active = parser.payload_buf[8];
                    uint32_t data_idx = 9;

                    uint32_t base_color = (is_active == 1) ? LCD_WHITE : LIGHT_GREY;
                    uint8_t base_r = (base_color >> 16) & 0xFF;
                    uint8_t base_g = (base_color >> 8) & 0xFF;
                    uint8_t base_b = base_color & 0xFF;

                    uint8_t bg_r = (global_theme_bg >> 16) & 0xFF;
                    uint8_t bg_g = (global_theme_bg >> 8) & 0xFF;
                    uint8_t bg_b = global_theme_bg & 0xFF;

                    LCD_SetLayer(1);

                    for (uint16_t y = 0; y < h; y++) {
                        for (uint16_t x = 0; x < w; x++) {
                            if (data_idx < parser.len) {
                                uint8_t alpha = parser.payload_buf[data_idx++];
                                if (alpha > 10) {
                                    if (x_off + (int16_t)x >= 0 && x_off + (int16_t)x < 800 && y_off + (int16_t)y >= 0 && y_off + (int16_t)y < 480) {
                                        uint8_t r = (base_r * alpha + bg_r * (255 - alpha)) / 255;
                                        uint8_t g = (base_g * alpha + bg_g * (255 - alpha)) / 255;
                                        uint8_t b = (base_b * alpha + bg_b * (255 - alpha)) / 255;
                                        Safe_DrawPoint_Layer1(x_off + x, y_off + y, r, g, b);
                                    }
                                }
                            }
                        }
                    }
                } else if (parser.type == 0x05) {
                    uint16_t progress_permille = parser.payload_buf[0] | (parser.payload_buf[1] << 8);

                    LCD_SetLayer(1);
                    LCD_SetColor(0x00000000);
                    LCD_FillRect(90, 440, 620, 30);

                    LCD_SetColor(LIGHT_GREY);
                    LCD_FillRect(100, 455, 600, 4);

                    uint16_t current_width = (600 * (uint32_t)progress_permille) / 1000;
                    if (current_width > 600) current_width = 600;

                    LCD_SetColor(LCD_WHITE);
                    LCD_FillRect(100, 455, current_width, 4);
                    LCD_FillCircle(100 + current_width, 457, 7);
                }
            }
            parser.state = STATE_HEAD1;
            break;

        default:
            parser.state = STATE_HEAD1;
            break;
    }
}
