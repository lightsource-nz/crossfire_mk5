#   crossfire on its production board, revision 0.1: an RP2350A hosting four USB-MIDI
# instruments through a hub on a PIO host pair, powered and addressed over one USB-C
# receptacle whose data pair is the chip's own controller in its device role -- the console,
# and the route into the bootloader. The board is pcb/, built and specified under
# pcb/releases/v0.1; its pico-sdk definition is boards/crossfire_v01.h and its wiring the
# light-board-crossfire-v01 crate. The application is the hardware-independent
# light_app_crossfire crate; this module is its production-board instantiation -- see
# rust/src/lib.rs. No assets: with no glass there is nothing to draw with, so the image is
# complete in itself and the data partition stays empty.
#   the executable name matches the module directory: light-flash.ps1 and light-debug.ps1
# resolve module/<target>/<target>.{uf2,elf}
add_executable(crossfire_v01
        ${LIGHT_PATH}/module/light_shell/src/main.c
)

target_link_libraries(crossfire_v01 PRIVATE
        pico_stdlib
        pico_multicore
        pico_bootrom
        light_app_crossfire_v01
)

#   THE SYSTEM CLOCK IS A CHOICE, AND THE SDK'S DEFAULT IS NOT ONE OF THE CHOICES. The PIO
# host clocks its transmitter at 48 MHz and its edge detector at 96 MHz from integer dividers,
# so the system clock must be a multiple of 12 MHz; the SDK's 150 MHz is not, and at it every
# bit on the host pair carries jitter. The board is designed for 240 MHz -- twenty cycles a
# bit, and the flash at its fastest legal clock through the divider of 2 -- but that is above
# the datasheet's 150, and whether it holds at temperature on stock core voltage is the first
# item on the board's list of things to prove. So the rate is one cache variable: 240000 by
# default, 192000 the next multiple down that still beats stock, 120000 the floor that always
# works. Anything else is refused here rather than discovered on the bus
set(CROSSFIRE_V01_SYS_CLK_KHZ 240000 CACHE STRING "crossfire_v01's system clock in kHz: 240000 (the design clock), 192000 or 120000 -- a multiple of 12 MHz, for the PIO host")
if(CROSSFIRE_V01_SYS_CLK_KHZ STREQUAL "240000")
        # 12 MHz XOSC * 120 = 1440 MHz VCO, / 6 / 1
        set(_crossfire_v01_vco 1440000000)
        set(_crossfire_v01_pd1 6)
        set(_crossfire_v01_pd2 1)
elseif(CROSSFIRE_V01_SYS_CLK_KHZ STREQUAL "192000")
        # 12 MHz XOSC * 96 = 1152 MHz VCO, / 6 / 1
        set(_crossfire_v01_vco 1152000000)
        set(_crossfire_v01_pd1 6)
        set(_crossfire_v01_pd2 1)
elseif(CROSSFIRE_V01_SYS_CLK_KHZ STREQUAL "120000")
        # 12 MHz XOSC * 120 = 1440 MHz VCO, / 6 / 2
        set(_crossfire_v01_vco 1440000000)
        set(_crossfire_v01_pd1 6)
        set(_crossfire_v01_pd2 2)
else()
        message(FATAL_ERROR "CROSSFIRE_V01_SYS_CLK_KHZ is '${CROSSFIRE_V01_SYS_CLK_KHZ}'; the PIO host needs 240000, 192000 or 120000")
endif()
target_compile_definitions(crossfire_v01 PRIVATE
        SYS_CLK_KHZ=${CROSSFIRE_V01_SYS_CLK_KHZ}
        PLL_SYS_VCO_FREQ_HZ=${_crossfire_v01_vco}
        PLL_SYS_POSTDIV1=${_crossfire_v01_pd1}
        PLL_SYS_POSTDIV2=${_crossfire_v01_pd2}
)

#   the image, signed and stamped with this project's own version: what the bootloader
# compares between the two slots, and what the ROM verifies before running it. Before
# light_shell_configure, which declares the outputs this post-processing feeds
light_seal_image(crossfire_v01 VERSION ${CROSSFIRE_VERSION})

light_shell_configure(crossfire_v01)

#   this device's bootloader: the image at the start of flash, carrying crossfire's flash map --
# the same A/B pair and data partition the Pico 2 build carries, on a part with four times
# the storage; growing the slots is a map change for when an image needs it
light_add_bootloader(crossfire_bootloader
        LAYOUT ${CMAKE_CURRENT_LIST_DIR}/partitions.json
)
