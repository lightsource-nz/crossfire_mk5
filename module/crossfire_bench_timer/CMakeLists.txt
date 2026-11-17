#   the bench's TIMER: an RP2350-Zero acting as a USB-MIDI instrument on one of crossfire's ports.
# It sends numbered messages through the board to the echo (crossfire_bench_echo) and times them:
# to the echo's wire, and to the echo itself coming back. Its figures are a block of RAM that
# scripts/bench.ps1 reads over SWD while it runs; the log goes to the UART on GP0. See
# crates/crossfire-bench for what is measured.
#   the executable name matches the module directory: light-flash.ps1 and light-debug.ps1
# resolve module/<target>/<target>.{uf2,elf}
add_executable(crossfire_bench_timer
        ${LIGHT_PATH}/module/light_shell/src/main.c
)

target_link_libraries(crossfire_bench_timer PRIVATE
        pico_stdlib
        pico_multicore
        pico_bootrom
        light_app_crossfire_bench_timer
)

light_shell_configure(crossfire_bench_timer)
