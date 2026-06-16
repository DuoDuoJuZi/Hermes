/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-27
 * @brief 串口数据协议解析源代码，负责解析接收到的指令并驱�?LCD 进行绘制
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
static uint32_t Pack_Layer1_Color(uint8_t r, uint8_t g, uint8_t b, uint8_t alpha) {
#if LCD_NUM_LAYERS == 2
    #if ColorMode_1 == LTDC_PIXEL_FORMAT_RGB565
        if (alpha == 0) return 0;
        return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
    #elif ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB1555
        if (alpha == 0) return 0;
        return 0x8000 | ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
    #elif ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB4444
        return ((alpha >> 4) << 12) | ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
    #else
        return ((uint32_t)alpha << 24) | ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
    #endif
#else
    return ((uint32_t)alpha << 24) | ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
#endif
}

static void Safe_DrawPoint_Layer1(uint16_t x, uint16_t y, uint8_t r, uint8_t g, uint8_t b) {
    LCD_DrawPoint(x, y, Pack_Layer1_Color(r, g, b, 255));
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

#define LYRIC_BITMAP_X 360
#define LYRIC_BITMAP_Y 115
#define LYRIC_BITMAP_W 440
#define LYRIC_BITMAP_H 305
#define LYRIC_PIXEL_ACTIVE_FLAG 0x80
#define LYRIC_PIXEL_LEVEL_MASK 0x7F
#define LYRIC_STAGE_PIXELS (LYRIC_BITMAP_W * LYRIC_BITMAP_H)
#define LYRIC_STAGE_BYTES (LYRIC_STAGE_PIXELS * BytesPerPixel_1)
#define LYRIC_STAGE_STRIDE_BYTES (((LYRIC_STAGE_BYTES + 31) / 32) * 32)
#define LYRIC_STAGE_A_ADDR (LCD_MemoryAdd + LCD_MemoryAdd_OFFSET + LCD_Width * LCD_Height * BytesPerPixel_1)
#define LYRIC_STAGE_B_ADDR (LYRIC_STAGE_A_ADDR + LYRIC_STAGE_STRIDE_BYTES)
#define LYRIC_STAGE_COMPOSE_ADDR (LYRIC_STAGE_B_ADDR + LYRIC_STAGE_STRIDE_BYTES)
#define LYRIC_LOCAL_ANIMATION_FRAMES 5
#define LYRIC_LOCAL_SCROLL_DISTANCE 42
#define COVER_MAX_W 280
#define COVER_MAX_H 400
#define COVER_MAX_PIXELS (COVER_MAX_W * COVER_MAX_H)
#define COVER_STAGE_BYTES (COVER_MAX_PIXELS * 2)
#define COVER_STAGE_STRIDE_BYTES (((COVER_STAGE_BYTES + 31) / 32) * 32)
#define COVER_NEXT_ADDR (LYRIC_STAGE_COMPOSE_ADDR + LYRIC_STAGE_STRIDE_BYTES)

static uint8_t lyric_front_buffer_index = 0;
static uint8_t lyric_front_buffer_valid = 0;

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
    lyric_front_buffer_valid = 0;

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
typedef struct {
    uint16_t width;
    uint16_t height;
    int16_t start_x;
    int16_t start_y;
    uint32_t theme;
    uint8_t valid;
} CoverState;

static CoverState cover_next = {0};

static uint16_t *Cover_NextBuffer(void) {
    return (uint16_t *)COVER_NEXT_ADDR;
}

static void Cover_ClearOverlayForNewTheme(void) {
    LCD_SetLayer(1);
    LCD_SetColor(0x00000000);
    LCD_FillRect(300, 380, 500, 60);
}

static void Cover_FillBackgroundExceptCover(const CoverState *state) {
    int16_t x0 = state->start_x;
    int16_t y0 = state->start_y;
    int16_t x1 = state->start_x + (int16_t)state->width;
    int16_t y1 = state->start_y + (int16_t)state->height;

    if (x0 < 0) x0 = 0;
    if (y0 < 0) y0 = 0;
    if (x1 > LCD_Width) x1 = LCD_Width;
    if (y1 > LCD_Height) y1 = LCD_Height;

    LCD_SetColor(state->theme);
    if (y0 > 0) {
        LCD_FillRect(0, 0, LCD_Width, y0);
    }
    if (y1 < LCD_Height) {
        LCD_FillRect(0, y1, LCD_Width, LCD_Height - y1);
    }
    if (x0 > 0 && y1 > y0) {
        LCD_FillRect(0, y0, x0, y1 - y0);
    }
    if (x1 < LCD_Width && y1 > y0) {
        LCD_FillRect(x1, y0, LCD_Width - x1, y1 - y0);
    }
}

static void Cover_DrawStaged(void) {
    if (!cover_next.valid || cover_next.width == 0 || cover_next.height == 0) {
        return;
    }

    uint16_t *next = Cover_NextBuffer();
    global_theme_bg = cover_next.theme;

    LCD_SetLayer(0);
    Cover_FillBackgroundExceptCover(&cover_next);

    for (uint16_t y = 0; y < cover_next.height; y++) {
        for (uint16_t x = 0; x < cover_next.width; x++) {
            int16_t draw_x = cover_next.start_x + (int16_t)x;
            int16_t draw_y = cover_next.start_y + (int16_t)y;
            if (draw_x >= 20 && draw_x < 300 && draw_y >= 50 && draw_y < 450) {
                uint16_t pixel = next[(uint32_t)y * cover_next.width + x];
#if ColorMode_0 == LTDC_PIXEL_FORMAT_RGB565
                LCD_DrawPoint(draw_x, draw_y, pixel);
#else
                uint8_t r = (uint8_t)(((pixel >> 11) & 0x1F) << 3);
                uint8_t g = (uint8_t)(((pixel >> 5) & 0x3F) << 2);
                uint8_t b = (uint8_t)((pixel & 0x1F) << 3);
                Safe_DrawPoint_Layer0(draw_x, draw_y, r, g, b);
#endif
            }
        }
    }

    Cover_ClearOverlayForNewTheme();
}

static void Draw_CoverRgb565Block(uint8_t *data, uint32_t length) {
    if (length < 11) {
        return;
    }

    uint16_t width = data[0] | (data[1] << 8);
    uint16_t height = data[2] | (data[3] << 8);
    uint8_t theme_r = data[4];
    uint8_t theme_g = data[5];
    uint8_t theme_b = data[6];
    uint16_t chunk_y = data[7] | (data[8] << 8);
    uint16_t chunk_h = data[9] | (data[10] << 8);
    uint32_t data_idx = 11;

    if (width == 0 || height == 0 || width > COVER_MAX_W || height > COVER_MAX_H || chunk_h == 0) {
        return;
    }
    if ((uint32_t)chunk_y + chunk_h > height) {
        return;
    }
    if (length < 11 + (uint32_t)width * chunk_h * 2) {
        return;
    }

    if (chunk_y == 0) {
        cover_next.width = width;
        cover_next.height = height;
        cover_next.start_x = 20 + (300 - 20 - (int16_t)width) / 2;
        if (cover_next.start_x < 20) cover_next.start_x = 20;
        cover_next.start_y = (480 - (int16_t)height) / 2 - 30;
        if (cover_next.start_y < 0) cover_next.start_y = 0;
        cover_next.theme = (0xFF << 24) | (theme_r << 16) | (theme_g << 8) | theme_b;
        cover_next.valid = 1;
    }

    if (!cover_next.valid || cover_next.width != width || cover_next.height != height) {
        return;
    }

    uint16_t *next = Cover_NextBuffer();
    for (uint16_t row = 0; row < chunk_h; row++) {
        uint16_t y = chunk_y + row;
        for (uint16_t x = 0; x < width; x++) {
            uint16_t pixel = data[data_idx] | (data[data_idx + 1] << 8);
            data_idx += 2;
            next[(uint32_t)y * width + x] = pixel;
        }
    }

    if ((uint32_t)chunk_y + chunk_h >= height) {
        Cover_DrawStaged();
    }
}
/**
 * @brief 绘制灰度歌词文本
 * @param data 文本点阵灰度像素及宽高数�?
 * @param length 数据总长�?
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
static uint16_t *Lyric_Buffer(uint8_t index) {
    return (uint16_t *)(index == 0 ? LYRIC_STAGE_A_ADDR : LYRIC_STAGE_B_ADDR);
}

static uint16_t *Lyric_ComposeBuffer(void) {
    return (uint16_t *)LYRIC_STAGE_COMPOSE_ADDR;
}

static void Lyric_CleanDCache(uint16_t *buffer) {
    SCB_CleanDCache_by_Addr((uint32_t *)buffer, LYRIC_STAGE_STRIDE_BYTES);
}

static void Lyric_ClearBuffer(uint16_t *buffer) {
    for (uint32_t i = 0; i < LYRIC_STAGE_PIXELS; i++) {
        buffer[i] = 0;
    }
}

static uint16_t Lyric_ConvertPixel(uint8_t encoded_pixel) {
    if (encoded_pixel == 0) {
        return 0;
    }

    uint8_t is_active = (encoded_pixel & LYRIC_PIXEL_ACTIVE_FLAG) != 0;
    uint8_t level = encoded_pixel & LYRIC_PIXEL_LEVEL_MASK;
    if (level == 0) {
        return 0;
    }

    uint32_t base_color = is_active ? LCD_WHITE : LIGHT_GREY;
    uint8_t base_r = (base_color >> 16) & 0xFF;
    uint8_t base_g = (base_color >> 8) & 0xFF;
    uint8_t base_b = base_color & 0xFF;
    uint8_t alpha = (uint8_t)(((uint16_t)level * 255 + 63) / 127);

#if ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB4444
    return (uint16_t)Pack_Layer1_Color(base_r, base_g, base_b, alpha);
#else
    uint8_t bg_r = (global_theme_bg >> 16) & 0xFF;
    uint8_t bg_g = (global_theme_bg >> 8) & 0xFF;
    uint8_t bg_b = global_theme_bg & 0xFF;
    uint8_t r = (base_r * alpha + bg_r * (255 - alpha)) / 255;
    uint8_t g = (base_g * alpha + bg_g * (255 - alpha)) / 255;
    uint8_t b = (base_b * alpha + bg_b * (255 - alpha)) / 255;
    return (uint16_t)Pack_Layer1_Color(r, g, b, 255);
#endif
}

static void Lyric_DrawPixelToBuffer(uint16_t *buffer, uint16_t dst_x, uint16_t dst_y, uint8_t encoded_pixel) {
    if (dst_x < LYRIC_BITMAP_X || dst_x >= LYRIC_BITMAP_X + LYRIC_BITMAP_W ||
        dst_y < LYRIC_BITMAP_Y || dst_y >= LYRIC_BITMAP_Y + LYRIC_BITMAP_H) {
        return;
    }

    uint16_t pixel = Lyric_ConvertPixel(encoded_pixel);
    if (pixel == 0) {
        return;
    }

    uint16_t local_x = dst_x - LYRIC_BITMAP_X;
    uint16_t local_y = dst_y - LYRIC_BITMAP_Y;
    buffer[(uint32_t)local_y * LYRIC_BITMAP_W + local_x] = pixel;
}

static void Lyric_CopyBufferToVisible(uint16_t *buffer) {
    Lyric_CleanDCache(buffer);

    while (READ_BIT(LTDC->CDSR, LTDC_CDSR_VDES) == 0U) {}
    while (READ_BIT(LTDC->CDSR, LTDC_CDSR_VDES) != 0U) {}

    DMA2D->CR &= ~DMA2D_CR_START;
    DMA2D->CR = DMA2D_M2M;
    DMA2D->FGPFCCR = ColorMode_1;
    DMA2D->OPFCCR = ColorMode_1;
    DMA2D->FGMAR = (uint32_t)buffer;
    DMA2D->OMAR = LCD_MemoryAdd + LCD_MemoryAdd_OFFSET + BytesPerPixel_1 * (LCD_Width * LYRIC_BITMAP_Y + LYRIC_BITMAP_X);
    DMA2D->FGOR = 0;
    DMA2D->OOR = LCD_Width - LYRIC_BITMAP_W;
    DMA2D->NLR = (LYRIC_BITMAP_W << 16) | LYRIC_BITMAP_H;
    DMA2D->CR |= DMA2D_CR_START;
    while (DMA2D->CR & DMA2D_CR_START) {}
}

static uint16_t Lyric_ScalePixelAlpha(uint16_t pixel, uint8_t weight) {
    if (pixel == 0 || weight == 0) {
        return 0;
    }

#if ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB4444
    uint16_t alpha = (pixel >> 12) & 0x0F;
    alpha = (alpha * weight + 127) / 255;
    if (alpha == 0) {
        return 0;
    }
    return (pixel & 0x0FFF) | (alpha << 12);
#elif ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB1555
    return weight >= 128 ? pixel : 0;
#else
    return pixel;
#endif
}

static uint8_t Lyric_PixelAlphaRank(uint16_t pixel) {
#if ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB4444
    return (uint8_t)((pixel >> 12) & 0x0F);
#elif ColorMode_1 == LTDC_PIXEL_FORMAT_ARGB1555
    return (pixel & 0x8000) ? 15 : 0;
#else
    return pixel == 0 ? 0 : 15;
#endif
}

static void Lyric_BlitShifted(uint16_t *dst, const uint16_t *src, int16_t y_offset, uint8_t weight) {
    for (uint16_t src_y = 0; src_y < LYRIC_BITMAP_H; src_y++) {
        int16_t dst_y = (int16_t)src_y + y_offset;
        if (dst_y < 0 || dst_y >= LYRIC_BITMAP_H) {
            continue;
        }

        uint32_t src_row = (uint32_t)src_y * LYRIC_BITMAP_W;
        uint32_t dst_row = (uint32_t)dst_y * LYRIC_BITMAP_W;
        for (uint16_t x = 0; x < LYRIC_BITMAP_W; x++) {
            uint16_t pixel = Lyric_ScalePixelAlpha(src[src_row + x], weight);
            if (pixel != 0 && Lyric_PixelAlphaRank(pixel) >= Lyric_PixelAlphaRank(dst[dst_row + x])) {
                dst[dst_row + x] = pixel;
            }
        }
    }
}

static void Lyric_CommitNextBuffer(uint8_t next_index, uint8_t animate) {
    uint16_t *next = Lyric_Buffer(next_index);

    if (!animate || !lyric_front_buffer_valid) {
        Lyric_CopyBufferToVisible(next);
        lyric_front_buffer_index = next_index;
        lyric_front_buffer_valid = 1;
        return;
    }

    uint16_t *previous = Lyric_Buffer(lyric_front_buffer_index);
    uint16_t *compose = Lyric_ComposeBuffer();

    for (uint8_t step = 1; step <= LYRIC_LOCAL_ANIMATION_FRAMES; step++) {
        uint16_t offset = (LYRIC_LOCAL_SCROLL_DISTANCE * step + LYRIC_LOCAL_ANIMATION_FRAMES / 2) / LYRIC_LOCAL_ANIMATION_FRAMES;
        uint8_t next_weight = (uint8_t)((255 * step + LYRIC_LOCAL_ANIMATION_FRAMES / 2) / LYRIC_LOCAL_ANIMATION_FRAMES);
        uint8_t previous_weight = 255 - next_weight;
        Lyric_ClearBuffer(compose);
        Lyric_BlitShifted(compose, previous, -(int16_t)offset, previous_weight);
        Lyric_BlitShifted(compose, next, (int16_t)(LYRIC_LOCAL_SCROLL_DISTANCE - offset), next_weight);
        Lyric_CopyBufferToVisible(compose);
    }

    Lyric_CopyBufferToVisible(next);
    lyric_front_buffer_index = next_index;
    lyric_front_buffer_valid = 1;
}

static void Draw_LyricBitmap(uint8_t *data, uint32_t length, uint8_t animate) {
    if (length < 9) {
        return;
    }

    uint16_t x = data[0] | (data[1] << 8);
    uint16_t y = data[2] | (data[3] << 8);
    uint16_t w = data[4] | (data[5] << 8);
    uint16_t h = data[6] | (data[7] << 8);
    uint8_t encoding = data[8];

    if (w == 0 || h == 0 || w > LYRIC_BITMAP_W || h > LYRIC_BITMAP_H) {
        return;
    }
    if (x < LYRIC_BITMAP_X || y < LYRIC_BITMAP_Y ||
        (uint32_t)x + w > LYRIC_BITMAP_X + LYRIC_BITMAP_W ||
        (uint32_t)y + h > LYRIC_BITMAP_Y + LYRIC_BITMAP_H) {
        return;
    }
    if (encoding != 0 && encoding != 1) {
        return;
    }
    if (encoding == 0 && length < 9 + (uint32_t)w * h) {
        return;
    }

    uint8_t next_index = lyric_front_buffer_index ^ 1;
    uint16_t *next = Lyric_Buffer(next_index);
    Lyric_ClearBuffer(next);

    if (encoding == 0) {
        uint32_t idx = 9;
        for (uint16_t row = 0; row < h; row++) {
            for (uint16_t col = 0; col < w; col++) {
                Lyric_DrawPixelToBuffer(next, x + col, y + row, data[idx++]);
            }
        }
    } else if (encoding == 1) {
        uint32_t idx = 9;
        uint32_t pixel_pos = 0;
        uint32_t total = (uint32_t)w * h;
        while (idx + 2 < length && pixel_pos < total) {
            uint16_t run_len = data[idx] | (data[idx + 1] << 8);
            uint8_t value = data[idx + 2];
            idx += 3;

            for (uint16_t i = 0; i < run_len && pixel_pos < total; i++, pixel_pos++) {
                uint16_t row = pixel_pos / w;
                uint16_t col = pixel_pos % w;
                Lyric_DrawPixelToBuffer(next, x + col, y + row, value);
            }
        }
    }

    Lyric_CommitNextBuffer(next_index, animate);
}
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
                } else if (parser.type == 0x06) {
                    Draw_LyricBitmap(parser.payload_buf, parser.len, 1);
                } else if (parser.type == 0x08) {
                    Draw_LyricBitmap(parser.payload_buf, parser.len, 0);
                } else if (parser.type == 0x07) {
                    Draw_CoverRgb565Block(parser.payload_buf, parser.len);
                }
            }
            parser.state = STATE_HEAD1;
            break;

        default:
            parser.state = STATE_HEAD1;
            break;
    }
}
