// crossfire, revision 0.1: the production board. An RP2350A hosting four USB-MIDI instruments
// through a hub, powered and addressed over one USB-C receptacle.
//
// This is the pico-sdk board definition: the facts the SDK's boot stage and runtime consume,
// and nothing else. The pin map belongs to the firmware, in the light-board-crossfire-v01
// crate, and the specification of the board as built is pcb/releases/v0.1/README.md. The
// values here are read off that specification, not assumed from a development board.
//
// -----------------------------------------------------
// NOTE: THIS HEADER IS ALSO INCLUDED BY ASSEMBLER SO
//       SHOULD ONLY CONSIST OF PREPROCESSOR DIRECTIVES
// -----------------------------------------------------

#ifndef _BOARDS_CROSSFIRE_V01_H
#define _BOARDS_CROSSFIRE_V01_H

pico_board_cmake_set(PICO_PLATFORM, rp2350)

// For board detection
#define CROSSFIRE_V01

// --- RP2350 VARIANT ---
// the 60-pin package: thirty GPIO, four of them analogue
#define PICO_RP2350A 1

// --- UART ---
// the console, on UART0's default pair, brought out on the debug header beside SWD
#ifndef PICO_DEFAULT_UART
#define PICO_DEFAULT_UART 0
#endif
#ifndef PICO_DEFAULT_UART_TX_PIN
#define PICO_DEFAULT_UART_TX_PIN 0
#endif
#ifndef PICO_DEFAULT_UART_RX_PIN
#define PICO_DEFAULT_UART_RX_PIN 1
#endif

// --- LED ---
// two indicators, each lit by driving its pin high through 330 ohms: red on GPIO 14,
// yellow-green on GPIO 15. The SDK's idea of "the" LED is the green one
#ifndef PICO_DEFAULT_LED_PIN
#define PICO_DEFAULT_LED_PIN 15
#endif
// no PICO_DEFAULT_WS2812_PIN

// --- I2C / SPI ---
// no defaults: the board wires neither bus. GPIO 4 and 5, I2C0's default pair, are free.

// --- FLASH ---
// W25Q128JVS, 16 MB, which the W25Q080 boot stage drives; a divider of 2 is what the flash
// runs at from the board's 240 MHz design clock (120 MHz, under the part's 133 MHz rating)
#define PICO_BOOT_STAGE2_CHOOSE_W25Q080 1

#ifndef PICO_FLASH_SPI_CLKDIV
#define PICO_FLASH_SPI_CLKDIV 2
#endif

pico_board_cmake_set_default(PICO_FLASH_SIZE_BYTES, (16 * 1024 * 1024))
#ifndef PICO_FLASH_SIZE_BYTES
#define PICO_FLASH_SIZE_BYTES (16 * 1024 * 1024)
#endif

// --- POWER ---
// no PICO_VBUS_PIN: VBUS is the board's only supply, so there is nothing to detect it against.
// no PICO_VSYS_PIN: nothing measures the rail. no PICO_SMPS_MODE_PIN: the 3.3 V rail comes
// from a fixed-output buck with no mode input.

pico_board_cmake_set_default(PICO_RP2350_A2_SUPPORTED, 1)
#ifndef PICO_RP2350_A2_SUPPORTED
#define PICO_RP2350_A2_SUPPORTED 1
#endif

#endif
