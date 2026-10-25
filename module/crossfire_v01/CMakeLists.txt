#   crossfire on its production board, revision 0.1: an RP2350A hosting four USB-MIDI
# instruments through a hub, powered and addressed over one USB-C receptacle. The board is
# pcb/, built and specified under pcb/releases/v0.1; its pico-sdk definition is
# boards/crossfire_v01.h and its wiring the light-board-crossfire-v01 crate.
#   TODAY this executable is the board's bring-up firmware -- console on both transports, the
# hub under console control, the indicators, the button and the CC readings -- until the
# application is wired to this hardware. See rust/src/lib.rs.
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

#   THE DESIGN CLOCK IS A CHOICE UNTIL IT IS PROVEN. The board is designed for 240 MHz --
# twenty cycles a bit for the PIO host, and the flash at its fastest legal clock through the
# divider of 2 -- and whether it holds at temperature on stock core voltage is the first item
# on the board's list of things to prove. So the SDK's 150 MHz default is what a build runs at
# unless asked, and asking is one cache variable rather than an edit
option(CROSSFIRE_V01_DESIGN_CLOCK "run crossfire_v01 at the board's 240 MHz design clock rather than the SDK's 150 MHz default" OFF)
if(CROSSFIRE_V01_DESIGN_CLOCK)
        target_compile_definitions(crossfire_v01 PRIVATE
                SYS_CLK_KHZ=240000
                # 12 MHz XOSC * 120 = 1440 MHz VCO, / 6 / 1 = 240 MHz
                PLL_SYS_VCO_FREQ_HZ=1440000000
                PLL_SYS_POSTDIV1=6
                PLL_SYS_POSTDIV2=1
        )
endif()

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
