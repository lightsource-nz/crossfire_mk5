#   the bench's ECHO: an RP2040-Zero acting as a USB-MIDI instrument on one of crossfire's ports.
# Every message that reaches it goes straight back, and its GP2 toggles as each arrives, for the
# timer (crossfire_bench_timer) to time. GP3 held to ground at power-up gives its OUT endpoint
# four bytes rather than sixty-four. It reports nothing: the timer measures everything.
#   the executable name matches the module directory: light-flash.ps1 and light-debug.ps1
# resolve module/<target>/<target>.{uf2,elf}
add_executable(crossfire_bench_echo
        ${LIGHT_PATH}/module/light_shell/src/main.c
)

target_link_libraries(crossfire_bench_echo PRIVATE
        pico_stdlib
        pico_multicore
        pico_bootrom
        light_app_crossfire_bench_echo
)

light_shell_configure(crossfire_bench_echo)
