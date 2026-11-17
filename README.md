# crossfire

USB-MIDI forwarding between every instrument on the host port: notes played on one instrument
reach all the others, with no computer in between.

crossfire runs on its own board, the **crossfire production board, revision 0.1** -- an RP2350
hosting four instruments through an on-board hub, powered and reached over one USB-C port, with
two indicators in place of a display. That is the default target of every script here. The same
application also runs on two development boards: a Pico 2 wearing the Pico-OLED-1.3, and the
stock RP2040 Pico, both with a small OLED status display.

## The production board, revision 0.1

Revision 0.1 went to the fabricator, assembled, in October 2026, and is the board the firmware
here runs on. Its design, and every decision behind it, is in `pcb/` (`pcb/README.md`); the files
that were submitted and a specification of the board as built are in `pcb/releases/v0.1/`. The
design files themselves have since moved on to revision 0.2.

The firmware target is `crossfire_v01`: its own pico-sdk board definition in `boards/`, its wiring
in `crates/light-board-crossfire-v01`, and the application instantiated in `module/crossfire_v01`.
Its instruments are hosted on a PIO pair to the on-board hub -- the framework's `usb-host-pio`
role, the same host stack the development boards run on their native controller -- and its USB-C
port is the device: the console rides it, as well as the debug header's UART, and so does the
1200-baud route into the bootloader, so the board is written over its own connector with no probe.
The build fixes the system clock at 240 MHz (`CROSSFIRE_V01_SYS_CLK_KHZ`; 192000 and 120000 are
the alternatives), because the PIO host wants a multiple of 12 MHz and the SDK's default is not
one. What it shares with the Pico 2 below the application -- probation, firmware
self-replacement -- is `crates/crossfire-rp2350`.

With no display, it shows its state on two indicators: green lit while an instrument is mounted
and interrupted by traffic, red lit while the supply is known to be short of what four ports could
ask for. It reads that from the receptacle's CC lines twice a second, since a worn cable's CC
contact can make and break after start-up (`power` on the console reports them on request),
releases the on-board hub once the host stack is listening and the supply's advertisement has held
for 100 ms, or after a second without one (`hub on|off` re-enumerates it or holds it), and `stats`
reports the PIO host's frame engine beside the application's own counters.

What has been shown on the board: the PIO host enumerates the hub and two USB-MIDI instruments
behind it, and notes played on them reach the board and are forwarded, with the activity indicator
following. The CC lines read all three advertisements correctly -- the USB default, 1.5 A and
3 A -- and the default and 3 A on either line, as the cable is turned over. A new image reaches it
over its console and keeps itself (below). `scripts/read-cc.ps1` reads the CC lines through the
debug header, which leaves the USB-C port free for the supply under test. Running from a USB-PD
supply, a keyboard played a synth through the board, notes and chords starting and stopping as
played, with the synth's controls reaching the keyboard the other way. `trace on` on the console
logs every message read from and written to each instrument, and `send` writes one to an instrument
directly. Not yet shown on hardware: the recovery of a hub port disabled for a device error, which
is tested on the host only.

## Building

This project is built against the **light framework** checkout beside it (`../light_mk5`); set
`LIGHT_PATH` for another layout. The framework supplies the C shell, the portable crates, the
ports, the asset compilers and the shared script layer. Everything that is crossfire's own --
the application, the board modules, the version, the signing key, the flash map -- is here.

```
scripts/build.ps1 [-Target crossfire_v01|crossfire_pico2|crossfire_pico]
scripts/flash.ps1                             # the production board, over its own USB-C port
scripts/update.ps1                            # the production board, while it runs (below)
scripts/console.ps1                           # its console, on the same port
scripts/flash.ps1 -Target crossfire_pico2     # a Pico, over SWD through a debug probe
scripts/debug.ps1 -Target crossfire_pico2
scripts/console.ps1 -Port <COM>               # a Pico's console rides the probe's UART
scripts/test.ps1                              # the application's host tests
```

With no `-Target`, the scripts build and write `crossfire_v01`, the production board.

CI (`.github/workflows/ci.yml`) runs the same scripts through the framework's reusable workflows:
the host tests, and a firmware build of each board -- the two RP2350 legs with their bootloader --
whose images are kept as artifacts.

## Running it without a board

```
scripts/sim.ps1 [-Release]                    # crossfire on a simulated board, in a window
```

`tools/crossfire-sim` runs the application crate -- the same one the firmware links, unchanged --
over the framework's simulated seams (`light_mk5/tools/light-sim`): an SH1107 panel that decodes
the real driver's commands onto a simulated glass, the indicator, the BOOTSEL button, a hub with
four sockets that instruments plug into and play from, and the console, in a desktop window. The
font, the look-and-feel and the interface are compiled at start-up by the same code the firmware
build runs, so the glass shows what the panel shows. Console lines can also be piped in on standard
input, and the log is echoed to standard output. What it simulates is the application's logic
against its seams, with a person at the controls; it is not a model of a chip, and the host tests
remain the place a module's behaviour is pinned down. The board it stands in for is a Pico with
the Pico-OLED-1.3; the production board's two indicators are not simulated yet.

## Images are signed

Every build signs its image and the device's flash map with `keys/dev.pem`, which is public and
protects nothing (see `keys/README.md`); a release is signed with a key that never appears here.
The flash map gives the application an A/B pair, so an update is written to the slot that is not
running and chosen on the next boot, and a data partition for the assets. What the hardware
verifies, and when, is specified in the framework's `documents/11-secure-boot-and-update.md`.

## A new image keeps itself only if it works

An update is started on probation: the RP2350's boot ROM runs it once, and unless it commits itself
the previous image boots again -- at the next reset, or when the ROM's probation watchdog runs out
some seventeen seconds after the boot. crossfire makes that decision on its own
(`crates/light_app_crossfire/src/probation.rs`). At boot the board asks the ROM whether the running
image is a candidate; if so, the application watches itself for three seconds after every module has
loaded and commits once the main loop is turning over, the console core is alive, and the display
has taken a whole frame with no bus timeout. A panic, a stopped loop or a failed check means no
commit, and so the old firmware comes back; a candidate that has not passed by ten seconds of uptime
is given up on well inside the ROM's window. The console reports every step as `probation: ...`.
The production board has no display and is not asked about one. The RP2040 has no second slot and
puts nothing on probation, so there every image is already kept.

An image reaches the other slot over the console, so a board that is running needs no BOOTSEL, no
button and no probe to be given new firmware: `scripts/update.ps1` builds the production board's
image, sends it over the board's own USB-C port a line at a time (the console commands `receive`
and `rx`), and follows the board through its reboot to say whether the new image kept itself and
which slot it is running. A 161 KiB image takes about fifteen seconds. The board has to be running
firmware that has these commands, which `scripts/flash.ps1` puts on once.

## Measuring a board

Two small boards plugged into crossfire's ports measure it the way an instrument sees it
(`crates/crossfire-bench`). The **timer**, an RP2350-Zero with a debug probe on its SWD pads, sends
numbered messages through the board. The **echo**, an RP2040-Zero, sends each one back and toggles a
wire to the timer as it arrives (GP2 to GP2, with a ground wire beside it). Nothing else should be
plugged into crossfire during a run. The timer records three things:

- latency one way, from sending a message to the echo's wire changing;
- the round trip, from sending to the echo arriving back;
- throughput, at rates from 500 to 24,000 messages a second, with every gap in the echoes counted.

```
scripts/flash.ps1 -Target crossfire_bench_echo    # once, with BOOT held as it is plugged into the computer
scripts/bench.ps1 -Flash                          # writes the timer over SWD, runs, prints the figures
scripts/bench.ps1                                 # another run
```

The echo's OUT endpoint takes sixty-four bytes. Hold its GP3 to ground at power-up to give it four
bytes instead, like an instrument that takes one event at a time. The timer's figures stay in RAM,
and the script reads them over SWD without halting anything; its log is on the UART at GP0.

## The development boards

Before the production board existed, crossfire was brought up on two Picos, and both are still
built and tested:

- **`crossfire_pico2`** -- a Pico 2 wearing the Pico-OLED-1.3. The same RP2350 as the production
  board, with the same bootloader, flash map and probation, but its instruments on the native USB
  port in its host role and an OLED status display. Flashed and debugged over SWD; its console
  rides the debug probe's UART.
- **`crossfire_pico`** -- the stock RP2040 Pico, with the same display. No second slot and no
  bootloader: its image is its whole flash. Flashed over SWD as well.

Both show their state on the display rather than on two indicators, and both need a debug probe,
which the production board does not.
