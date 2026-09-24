#   crossfire on a Pico 2 wearing the Waveshare Pico-OLED-1.3 display board: the native USB
# port HOSTS USB-MIDI instruments (through an OTG adapter, one device or a hub), forwarding
# every instrument's MIDI to every other, with the OLED as the status display and the
# console on the UART (GPIO 0/1). The application is the hardware-independent
# light_app_crossfire crate; this module is its Pico 2 instantiation -- see
# rust/src/board.rs.
#   the executable name matches the module directory: light-flash.ps1 and light-debug.ps1
# resolve module/<target>/<target>.{uf2,elf}
add_executable(crossfire_pico2
        ${LIGHT_PATH}/module/light_shell/src/main.c
)

target_link_libraries(crossfire_pico2 PRIVATE
        pico_stdlib
        pico_multicore
        pico_bootrom
        light_app_crossfire_pico2
)

#   THE ASSETS GO IN THE DATA PARTITION, not in the image. Each of the three is compiled exactly
# as before -- what changes is that none of them names a crate to embed it in, so the blobs are
# gathered into one pack below instead.
#
#   the status font: 16 px on the Pico-OLED-1.3's real geometry (64x128 over 17.2x32.3 mm)
# -- two lines of ten cells across the rotated 128-pixel row
light_add_font(crossfire_pico2_font
        FONT ${CMAKE_CURRENT_LIST_DIR}/../../assets/fonts/TypeLightSans.ttf
        DISPLAY po13 WIDTH 64 HEIGHT 128 DIMENSION 17.2x32.3
        POINT_SIZE 12 PIXEL_SIZE 16
)

#   the look-and-feel and the interface as data, shared with the Pico crossfire: the framework
# MONO default with this OLED's rounding, and the crossfire status page -- both live beside the
# application crate
light_add_theme(crossfire_pico2_theme MONO
        THEME ${CMAKE_CURRENT_LIST_DIR}/../../crates/light_app_crossfire/theme.json
)
light_add_ui(crossfire_pico2_ui
        UI ${CMAKE_CURRENT_LIST_DIR}/../../crates/light_app_crossfire/design.json
)

#   the three of them as one pack, written to the partition that accepts the data family and read
# from there at startup. The crate is built with the pack's DIGEST rather than its contents: the
# image is signed, so the pack is trusted at one remove, and assets can be replaced on a device
# without re-signing anything. The entry names are what the firmware asks for
light_add_asset_pack(crossfire_pico2_assets
        ENTRIES font=crossfire_pico2_font theme=crossfire_pico2_theme ui=crossfire_pico2_ui
        CRATE light_app_crossfire_pico2 ENV LIGHT_ASSETS_SHA256
        FAMILY data
)

#   the application image, signed and stamped with this project's own version: what the
# bootloader compares between the two slots, and what the ROM verifies before running it.
# Before light_shell_configure, which declares the outputs this post-processing feeds
light_seal_image(crossfire_pico2 VERSION ${CROSSFIRE_VERSION})

light_shell_configure(crossfire_pico2)

#   this device's bootloader: the image at the start of flash, carrying crossfire's flash map --
# an A/B pair for the application, so an update is written to the slot that is not running and
# chosen on the next boot, and a data partition for the assets. The framework owns the code, this
# project owns the map -- see the framework's documents/11-secure-boot-and-update.md
light_add_bootloader(crossfire_bootloader
        LAYOUT ${CMAKE_CURRENT_LIST_DIR}/partitions.json
)
