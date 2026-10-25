# crossfire

USB-MIDI forwarding between every instrument on the host port. Three tangible boards run it: the
production board (`pcb/`, four instruments through a hub, two indicators), a Pico 2 wearing the
Pico-OLED-1.3, and the stock RP2040 Pico -- the last two with a small OLED status display.

## Building

This project is built against the **light framework** checkout beside it (`../light_mk5`); set
`LIGHT_PATH` for another layout. The framework supplies the C shell, the portable crates, the
ports, the asset compilers and the shared script layer. Everything that is crossfire's own --
the application, the board modules, the version, the signing key, the flash map -- is here.

```
scripts/build.ps1 [-Target crossfire_pico2|crossfire_pico|crossfire_v01]
scripts/flash.ps1 -Target crossfire_pico2     # over SWD, through a debug probe
scripts/debug.ps1 -Target crossfire_pico2
scripts/console.ps1 -Port <COM>               # the console rides the probe's UART
scripts/test.ps1                              # the application's host tests
```

`crossfire_v01` is the production board, revision 0.1 (`pcb/`): its own pico-sdk board
definition in `boards/`, its wiring in `crates/light-board-crossfire-v01`, and the application
instantiated in `module/crossfire_v01`. Its instruments are hosted on a PIO pair to the on-board
hub -- the framework's `usb-host-pio` role, the same host stack the other boards run on their
native controller -- and its USB-C port is the device: the console rides it, as well as the debug
header's UART, and so does the 1200-baud route into the bootloader. The build fixes the system
clock at 240 MHz (`CROSSFIRE_V01_SYS_CLK_KHZ`; 192000 and 120000 are the alternatives), because
the PIO host wants a multiple of 12 MHz and the SDK's default is not one. What the two RP2350
boards share below the application -- probation, firmware self-replacement -- is
`crates/crossfire-rp2350`.

The production board has no display, so it brings two indicators instead: green lit while an
instrument is mounted and interrupted by traffic, red lit while the supply is known to be short
of what four ports could ask for. It reads that from the receptacle's CC lines at start-up
(`power` on the console reads them again), releases the on-board hub once the host stack is
listening (`hub on|off` re-enumerates it or holds it), and `stats` reports the PIO host's frame
engine beside the application's own counters. None of this has run on hardware yet: the boards
are in production, and the PIO host in particular is written against the reference
implementation's timings and awaits its first bus.

CI (`.github/workflows/ci.yml`) runs the same scripts through the framework's reusable workflows:
the host tests, and a firmware build of each board -- the two RP2350 legs with their bootloader --
whose images are kept as artifacts.

## Images are signed

Every build signs its image and the device's flash map with `keys/dev.pem`, which is public and
protects nothing (see `keys/README.md`); a release is signed with a key that never appears here.
The flash map gives the application an A/B pair, so an update is written to the slot that is not
running and chosen on the next boot, and a data partition for the assets. What the hardware
verifies, and when, is specified in the framework's `documents/11-secure-boot-and-update.md`.

## A new image keeps itself only if it works

An update is started on probation: the Pico 2's boot ROM runs it once, and unless it commits itself
the previous image boots again -- at the next reset, or when the ROM's probation watchdog runs out
some seventeen seconds after the boot. crossfire makes that decision on its own
(`crates/light_app_crossfire/src/probation.rs`). At boot the board asks the ROM whether the running
image is a candidate; if so, the application watches itself for three seconds after every module has
loaded and commits once the main loop is turning over, the console core is alive, and the display
has taken a whole frame with no bus timeout. A panic, a stopped loop or a failed check means no
commit, and so the old firmware comes back; a candidate that has not passed by ten seconds of uptime
is given up on well inside the ROM's window. The console reports every step as `probation: ...`.
A board with no display is not asked about one. The RP2040 has no second slot and puts nothing on
probation, so there every image is already kept.

## The production board

`pcb/` holds the hardware for the board this firmware is heading for -- an RP2350 hosting four
instruments through a hub, powered over USB-C. It is a KiCad project, designed, routed and ordered:
revision 0.1 went to the fabricator, assembled, in October 2026. `pcb/README.md` carries the
design and every decision behind it; `pcb/releases/v0.1/` carries the files that were submitted
and a specification of the board as built. Nothing has been measured yet.
