# crossfire

USB-MIDI forwarding between every instrument on the host port, with a small OLED status display.
Two tangible boards run it: a Pico 2 wearing the Pico-OLED-1.3, and the stock RP2040 Pico.

## Building

This project is built against the **light framework** checkout beside it (`../light_mk5`); set
`LIGHT_PATH` for another layout. The framework supplies the C shell, the portable crates, the
ports, the asset compilers and the shared script layer. Everything that is crossfire's own --
the application, the board modules, the version, the signing key, the flash map -- is here.

```
scripts/build.ps1 [-Target crossfire_pico2|crossfire_pico]
scripts/flash.ps1 -Target crossfire_pico2     # over SWD, through a debug probe
scripts/debug.ps1 -Target crossfire_pico2
scripts/console.ps1 -Port <COM>               # the console rides the probe's UART
scripts/test.ps1                              # the application's host tests
```

## Images are signed

Every build signs its image and the device's flash map with `keys/dev.pem`, which is public and
protects nothing (see `keys/README.md`); a release is signed with a key that never appears here.
The flash map gives the application an A/B pair, so an update is written to the slot that is not
running and chosen on the next boot, and a data partition for the assets. What the hardware
verifies, and when, is specified in the framework's `documents/11-secure-boot-and-update.md`.
