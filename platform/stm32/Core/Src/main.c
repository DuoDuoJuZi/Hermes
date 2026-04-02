/***
	*******************************************************************************************************
	*	@file  	main.c
	*	@version V1.0
	*  @date    2022-7-12
	*	@author  反客科技	
	*	@brief   驱动RGB显示屏进行显�?
   *******************************************************************************************************
   *  @description
	*
	*	实验平台：反客STM32H750XBH6核心�?（型号：FK750M5-XBH6�? 反客800*480分辨率的RGB屏幕
	*	淘宝地址：https://shop212360197.taobao.com
	*	QQ交流群：536665479
	*
>>>>> 功能说明�?
	*
	*	1. 使用LTDC驱动RGB屏幕
	*	2.	进行简单的功能演示
	*
	*******************************************************************************************************
***/


#include "main.h"
#include "led.h"
#include "usart.h"
#include "sdram.h"  
#include "lcd_rgb.h"
#include "lcd_pwm.h"
#include "lcd_test.h"
#include "protocol_parser.h"
#include "usb_device.h"
#include "touch_800x480.h"
#include "usbd_cdc_if.h"

#define UART_RX_BUF_SIZE 16384
uint8_t uart_rx_buf[UART_RX_BUF_SIZE];
volatile uint32_t uart_rx_head = 0;
volatile uint32_t uart_rx_tail = 0;

uint8_t rx_data;
extern UART_HandleTypeDef huart1;
/********************************************** 函数声明 *******************************************/

void SystemClock_Config(void);		// 时钟初始�?
void MPU_Config(void);					// MPU配置
	
/***************************************************************************************************
*	�?�?�? main
*	入口参数: �?
*	�?�?�? �?
*	函数功能: LTDC驱动屏幕测试
*	�?   �? �?
****************************************************************************************************/

int main(void)
{
	MPU_Config();				// MPU配置
	SCB_EnableICache();		// 使能ICache
	SCB_EnableDCache();		// 使能DCache
	HAL_Init();					// 初始化HAL�?
	SystemClock_Config();	// 配置系统时钟，主�?80MHz
	LED_Init();					// 初始化LED引脚
	USART1_Init();				// USART1初始�?
	MX_FMC_Init();				// SDRAM初始�?
	
  MX_LTDC_Init();
  MX_USB_DEVICE_Init();
  Touch_Init();

  LCD_SetLayer(1);
  LCD_SetColor(0x00000000);
  LCD_FillRect(0, 0, 800, 480);
  LCD_SetLayer(0);
  LCD_SetColor(0xFF000000);
  LCD_FillRect(0, 0, 800, 480);

  Protocol_Init();
  HAL_UART_Receive_IT(&huart1, &rx_data, 1);
  uint8_t last_touch_flag = 0;
  while (1)
  {
    while (uart_rx_tail != uart_rx_head)
    {
      uint8_t byte = uart_rx_buf[uart_rx_tail];
      uart_rx_tail = (uart_rx_tail + 1) % UART_RX_BUF_SIZE;
      Protocol_ParseByte(byte);
    }
    Touch_Scan();
    static uint16_t last_permille = 0xFFFF;
    
    if (touchInfo.flag == 1) {
      if (touchInfo.y[0] >= 440 && touchInfo.y[0] <= 480) {
        uint16_t x = touchInfo.x[0];
        if (x < 100) x = 100;
        if (x > 700) x = 700;
        uint16_t permille = (x - 100) * 1000 / 600;

        uint16_t diff = (permille > last_permille) ? (permille - last_permille) : (last_permille - permille);
        if (last_permille == 0xFFFF || diff >= 5) {
          uint8_t packet[3] = {'S', permille & 0xFF, (permille >> 8) & 0xFF};
          CDC_Transmit_FS(packet, 3);
          last_permille = permille;
        }
      } else if (last_touch_flag == 0) {
        if (touchInfo.x[0] > 320 && touchInfo.y[0] < 440) {
          uint16_t y = touchInfo.y[0];
          uint8_t packet[3] = {'C', y & 0xFF, (y >> 8) & 0xFF};
          CDC_Transmit_FS(packet, 3);
        } else if (touchInfo.y[0] > 360 && touchInfo.y[0] < 440) {
          if (touchInfo.x[0] > 120 && touchInfo.x[0] < 200) {
            uint8_t cmd = 'P';
            CDC_Transmit_FS(&cmd, 1);
          } else if (touchInfo.x[0] > 40 && touchInfo.x[0] < 120) {
            uint8_t cmd = 'L';
            CDC_Transmit_FS(&cmd, 1);
          } else if (touchInfo.x[0] > 200 && touchInfo.x[0] < 280) {
            uint8_t cmd = 'N';
            CDC_Transmit_FS(&cmd, 1);
          } else {
            uint8_t cmd_fallback = 'E';
            CDC_Transmit_FS(&cmd_fallback, 1);
          }
        } else {
          uint8_t cmd_fallback = 'E';
          CDC_Transmit_FS(&cmd_fallback, 1);
        }
      }
    } else {
      last_permille = 0xFFFF;
    }
    last_touch_flag = touchInfo.flag;

// #if LCD_NUM_LAYERS == 2				// 如果定义了双层，则开启双层显示测�?
		
// 		LCD_Test_DoubleLayer();
		
// #endif		
// 		LCD_Test_Clear();			// 清屏测试
// 		LCD_Test_Text();			// 文本显示测试
// 		LCD_Test_Variable();		// 变量显示，包括整数和小数
// 		LCD_Test_Color();			// 颜色测试
// 		LCD_Test_GrahicTest();	// 2D图形绘制
// 		LCD_Test_FillRect();		// 矩形填充测试
// 		LCD_Test_Image();			// 图片显示测试
		
// 		LCD_Test_Vertical();		// 垂直显示测试	
	}
}
/****************************************************************************************************/
/**
  * @brief  System Clock Configuration
  *         The system Clock is configured as follow : 
  *            System Clock source            = PLL (HSE)
  *            SYSCLK(Hz)                     = 480000000 (CPU Clock)
  *            HCLK(Hz)                       = 240000000 (AXI and AHBs Clock)
  *            AHB Prescaler                  = 2
  *            D1 APB3 Prescaler              = 2 (APB3 Clock  120MHz)
  *            D2 APB1 Prescaler              = 2 (APB1 Clock  120MHz)
  *            D2 APB2 Prescaler              = 2 (APB2 Clock  120MHz)
  *            D3 APB4 Prescaler              = 2 (APB4 Clock  120MHz)
  *            HSE Frequency(Hz)              = 25000000
  *            PLL_M                          = 5
  *            PLL_N                          = 192
  *            PLL_P                          = 2
  *            PLL_Q                          = 2
  *            PLL_R                          = 2
  *            VDD(V)                         = 3.3
  *            Flash Latency(WS)              = 4
  * @param  None
  * @retval None
  */
/****************************************************************************************************/  
void SystemClock_Config(void)
{
  RCC_OscInitTypeDef RCC_OscInitStruct = {0};
  RCC_ClkInitTypeDef RCC_ClkInitStruct = {0};
  RCC_PeriphCLKInitTypeDef PeriphClkInitStruct = {0};
  
  /** Supply configuration update enable
  */
  HAL_PWREx_ConfigSupply(PWR_LDO_SUPPLY);

  /** Configure the main internal regulator output voltage
  */
  __HAL_PWR_VOLTAGESCALING_CONFIG(PWR_REGULATOR_VOLTAGE_SCALE1);

  while(!__HAL_PWR_GET_FLAG(PWR_FLAG_VOSRDY)) {}

  __HAL_RCC_SYSCFG_CLK_ENABLE();
  __HAL_PWR_VOLTAGESCALING_CONFIG(PWR_REGULATOR_VOLTAGE_SCALE0);

  while(!__HAL_PWR_GET_FLAG(PWR_FLAG_VOSRDY)) {}

  /** Macro to configure the PLL clock source
  */
  __HAL_RCC_PLL_PLLSOURCE_CONFIG(RCC_PLLSOURCE_HSE);

  /** Initializes the RCC Oscillators according to the specified parameters
  * in the RCC_OscInitTypeDef structure.
  */
  RCC_OscInitStruct.OscillatorType = RCC_OSCILLATORTYPE_HSI48|RCC_OSCILLATORTYPE_HSE;
  RCC_OscInitStruct.HSEState = RCC_HSE_ON;
  RCC_OscInitStruct.HSI48State = RCC_HSI48_ON;
  RCC_OscInitStruct.PLL.PLLState = RCC_PLL_ON;
  RCC_OscInitStruct.PLL.PLLSource = RCC_PLLSOURCE_HSE;
  RCC_OscInitStruct.PLL.PLLM = 5;
  RCC_OscInitStruct.PLL.PLLN = 192;
  RCC_OscInitStruct.PLL.PLLP = 2;
  RCC_OscInitStruct.PLL.PLLQ = 2;
  RCC_OscInitStruct.PLL.PLLR = 2;
  RCC_OscInitStruct.PLL.PLLRGE = RCC_PLL1VCIRANGE_2;
  RCC_OscInitStruct.PLL.PLLVCOSEL = RCC_PLL1VCOWIDE;
  RCC_OscInitStruct.PLL.PLLFRACN = 0;
  if (HAL_RCC_OscConfig(&RCC_OscInitStruct) != HAL_OK)
  {
    Error_Handler();
  }

  /** Initializes the CPU, AHB and APB buses clocks
  */
  RCC_ClkInitStruct.ClockType = RCC_CLOCKTYPE_HCLK|RCC_CLOCKTYPE_SYSCLK
                              |RCC_CLOCKTYPE_PCLK1|RCC_CLOCKTYPE_PCLK2
                              |RCC_CLOCKTYPE_D3PCLK1|RCC_CLOCKTYPE_D1PCLK1;
  RCC_ClkInitStruct.SYSCLKSource = RCC_SYSCLKSOURCE_PLLCLK;
  RCC_ClkInitStruct.SYSCLKDivider = RCC_SYSCLK_DIV1;
  RCC_ClkInitStruct.AHBCLKDivider = RCC_HCLK_DIV2;
  RCC_ClkInitStruct.APB3CLKDivider = RCC_APB3_DIV2;
  RCC_ClkInitStruct.APB1CLKDivider = RCC_APB1_DIV2;
  RCC_ClkInitStruct.APB2CLKDivider = RCC_APB2_DIV2;
  RCC_ClkInitStruct.APB4CLKDivider = RCC_APB4_DIV2;

  if (HAL_RCC_ClockConfig(&RCC_ClkInitStruct, FLASH_LATENCY_4) != HAL_OK)
  {
    Error_Handler();
  }
  
  /* 设置LTDC时钟，这里设置为33MHz，即刷新率在60帧左右，过高或者过低都会造成闪烁 */
  /* LCD clock configuration */
  /* PLL3_VCO Input = HSE_VALUE/PLL3M = 1 Mhz */
  /* PLL3_VCO Output = PLL3_VCO Input * PLL3N = 330 Mhz */
  /* PLLLCDCLK = PLL3_VCO Output/PLL3R = 330/10 = 33 Mhz */
  /* LTDC clock frequency = PLLLCDCLK = 33 Mhz */  
   
      
  PeriphClkInitStruct.PLL3.PLL3M = 25;
  PeriphClkInitStruct.PLL3.PLL3N = 330;
  PeriphClkInitStruct.PLL3.PLL3P = 2;
  PeriphClkInitStruct.PLL3.PLL3Q = 2;
  PeriphClkInitStruct.PLL3.PLL3R = 10;
  PeriphClkInitStruct.PLL3.PLL3RGE = RCC_PLL3VCIRANGE_0;
  PeriphClkInitStruct.PLL3.PLL3VCOSEL = RCC_PLL3VCOMEDIUM;
  PeriphClkInitStruct.PLL3.PLL3FRACN = 0;
  
  PeriphClkInitStruct.PeriphClockSelection = RCC_PERIPHCLK_LTDC|RCC_PERIPHCLK_USART1|RCC_PERIPHCLK_FMC|RCC_PERIPHCLK_USB;               
  PeriphClkInitStruct.FmcClockSelection = RCC_FMCCLKSOURCE_D1HCLK;
  PeriphClkInitStruct.Usart16ClockSelection = RCC_USART16CLKSOURCE_D2PCLK2;
  PeriphClkInitStruct.UsbClockSelection = RCC_USBCLKSOURCE_HSI48;
  if (HAL_RCCEx_PeriphCLKConfig(&PeriphClkInitStruct) != HAL_OK)
  {
    Error_Handler();
  }

  /** Enable USB Voltage detector
  */
  HAL_PWREx_EnableUSBVoltageDetector();
}


//	配置MPU
//
void MPU_Config(void)
{
	MPU_Region_InitTypeDef MPU_InitStruct;

	HAL_MPU_Disable();		// 先禁止MPU

	MPU_InitStruct.Enable           = MPU_REGION_ENABLE;
	MPU_InitStruct.BaseAddress      = SDRAM_BANK_ADDR;
	MPU_InitStruct.Size             = MPU_REGION_SIZE_32MB;
	MPU_InitStruct.AccessPermission = MPU_REGION_FULL_ACCESS;
	MPU_InitStruct.IsBufferable     = MPU_ACCESS_NOT_BUFFERABLE;
	MPU_InitStruct.IsCacheable      = MPU_ACCESS_CACHEABLE;
	MPU_InitStruct.IsShareable      = MPU_ACCESS_NOT_SHAREABLE;
	MPU_InitStruct.Number           = MPU_REGION_NUMBER2;
	MPU_InitStruct.TypeExtField     = MPU_TEX_LEVEL0;
	MPU_InitStruct.SubRegionDisable = 0x00;
	MPU_InitStruct.DisableExec      = MPU_INSTRUCTION_ACCESS_ENABLE;

	HAL_MPU_ConfigRegion(&MPU_InitStruct);

	HAL_MPU_Enable(MPU_PRIVILEGED_DEFAULT);	// 使能MPU
}


/**
  * @brief  This function is executed in case of error occurrence.
  * @retval None
  */
void Error_Handler(void)
{
  /* USER CODE BEGIN Error_Handler_Debug */
  /* User can add his own implementation to report the HAL error return state */
  __disable_irq();
  while (1)
        {
                while (uart_rx_tail != uart_rx_head)
                {
                        uint8_t byte = uart_rx_buf[uart_rx_tail];
                        uart_rx_tail = (uart_rx_tail + 1) % UART_RX_BUF_SIZE;
                        Protocol_ParseByte(byte);
                }
  }
  /* USER CODE END Error_Handler_Debug */
}
/**
 * @brief 串口接收完成回调函数�?
 * @param huart 串口句柄指针�?
 * @return 无，
 */
void HAL_UART_RxCpltCallback(UART_HandleTypeDef *huart) {
    if (huart->Instance == USART1) {
        uart_rx_buf[uart_rx_head] = rx_data;
        uart_rx_head = (uart_rx_head + 1) % UART_RX_BUF_SIZE;
        HAL_UART_Receive_IT(&huart1, &rx_data, 1);
    }
}

