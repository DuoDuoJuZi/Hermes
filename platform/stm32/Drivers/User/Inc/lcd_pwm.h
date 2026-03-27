#ifndef __PWM_H
#define __PWM_H 
 
 
#include "stm32h7xx_hal.h"
#include "lcd_rgb.h" 
 
#define  LTDC_PWM_PIN								GPIO_PIN_6								// 引脚			
#define	LTDC_PWM_PORT								GPIOH										// 端口
#define 	GPIO_LTDC_PWM_CLK_ENABLE        	   __HAL_RCC_GPIOH_CLK_ENABLE()	 	// 时钟
#define  LCD_PWM_AF           					GPIO_AF2_TIM12							// 复用
	
#define  LTDC_PWM_TIM								TIM12										// 定时器
#define  LTDC_PWM_TIM_CLK_ENABLE					__HAL_RCC_TIM12_CLK_ENABLE()		// 时钟
#define  LTDC_PWM_TIM_CHANNEL					 	TIM_CHANNEL_1							// 通道


/*-------------------- 函数声明 ----------------------*/

void LCD_PWMinit(uint8_t pulse);			 //PWM初始化
void LCD_PwmSetPulse (uint8_t pulse);	 //设置占空比


#endif //__PWM_H



