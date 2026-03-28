/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-27
 * @brief ��������Э�����ͷ�ļ���?
 */
#ifndef __PROTOCOL_PARSER_H
#define __PROTOCOL_PARSER_H

#include "stdint.h"

/**
 * @brief ��ʼ��Э�����״�?����
 * @param �ޣ�
 * @return �ޣ�
 */
void Protocol_Init(void);

/**
 * @brief ���������ֽڲ�����״̬����
 * @param byte ���յ��ĵ����ֽ����ݣ�
 * @return �ޣ�
 */
void Protocol_ParseByte(uint8_t byte);

#endif