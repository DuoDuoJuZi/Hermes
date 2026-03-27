/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-27
 * @brief 串口数据协议解析头文件，
 */
#ifndef __PROTOCOL_PARSER_H
#define __PROTOCOL_PARSER_H

#include "stdint.h"

/**
 * @brief 初始化协议解析状态机，
 * @param 无，
 * @return 无，
 */
void Protocol_Init(void);

/**
 * @brief 解析单个字节并驱动状态机，
 * @param byte 接收到的单个字节数据，
 * @return 无，
 */
void Protocol_ParseByte(uint8_t byte);

#endif