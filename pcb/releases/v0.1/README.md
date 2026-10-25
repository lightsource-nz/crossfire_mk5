# crossfire hardware, revision 0.1

The first board built. Four USB-MIDI instruments hosted through a hub by an RP2350A, powered and
addressed over one USB-C connector. This folder is the record of what was ordered: the files as
they were submitted, and the specification of the board they describe. The design and the reasons
behind it are in `../../README.md`; this file states what the hardware *is*.

**Ordered October 2026 from JLCPCB, fabricated and assembled.** Five boards, four layers, ENIG,
assembled on the front face from the bill of materials and placement file here. The files were
exported from the design at commit `8ae5b26`, which is the state the title block calls revision
0.1 and the silkscreen prints as `CROSSFIRE Rev 0.1`.

## The files

| file | what it is |
| --- | --- |
| `crossfire-gerbers.zip` | copper (four layers), solder mask and paste (front and back), silkscreen (front and back), board outline, drill file with its map, and the job file. SHA-256 `320e0808 30e811bc c5713f28 18175ef2 3816b72d 0167c421 23c93d77 7e59ac8c` |
| `crossfire-bom.csv` | the bill of materials as uploaded: reference, value, package, part name, catalogue number |
| `crossfire-cpl.csv` | the placement file as uploaded: part centres, face, rotation, in the assembler's column names |
| `drc.rpt` | the rules check that ran on the board before export, with schematic parity: clean |
| `bom-as-ordered.csv` | the assembler's own bill, exported from the order: what was fitted, line by line, with the catalogue number chosen for every part and the quantity bought for five boards |

All four were written by `scripts/export-fab.ps1` from the board, and are reproducible from the
source at that commit. They are kept here because a built board is a fact that the source no
longer describes exactly: the source moves on, and this is what was made.

**One difference between these files and the boards.** The fabricator's engineers, checking the
files before production, opened up the solder mask round three of the upstream receptacle's four
through-hole shell slots on the back face, where the files drew the opening the same size as the
copper. The boards carry their correction. The source has carried the same margin since the first change
after these files were exported, the commit "J1: a solder mask margin on the shell slots", so
files exported from it differ from the zip here in exactly that respect.

**Where the boards differ from the bill as exported.** The order was placed from
`crossfire-bom.csv`, and `bom-as-ordered.csv` is what the assembler fitted. Three things differ:

- **L1, the core inductor, is TDK VLS201610ET-3R3M, C114768**, not the Abracon the bill names,
  which the catalogue could not supply. It sits on the same land and is the weaker part, 730 mA
  and 458 mΩ against 2.4 A and 115 mΩ, which the regulator's 200 mA tolerates.
- **C39 to C42, the 22 µF port capacitors, are not fitted.** The catalogue had none in 1210 at
  the voltage. The pads are bare and take a hand-fitted part; Samsung CL32B226KAJNNNE, C309062,
  is the one to fit.
- **Every passive carries the catalogue number the assembler's matcher chose**, where the bill
  left the choice to it: 100 nF C1525, 10 µF C15850, 22 µF 0603 C59461, 15 pF C1548, 18 pF
  C1647, and the resistors C25744 (10 kΩ), C25741 (100 kΩ), C25076 (100 Ω), C11702 (1 kΩ),
  C25905 (5.1 kΩ), C25752 (12 kΩ), C25756 (15 kΩ).

L2 is the Sunlord SWPA4030S4R7MT the bill names, on the land that takes it and the Coilcraft
XEL4030-472MEC alike. The debug header J2, the eight test pads and the mounting holes are not
fitted, by design. The schematic has carried all of the above since the commit after this record,
so the next bill exported from it names what these boards carry.

## What the board is

| | |
| --- | --- |
| processor | RP2350A, QFN-60, both cores available, 520 KB SRAM; system clock designed for 240 MHz from a 12 MHz crystal, QSPI at 120 MHz |
| flash | W25Q128JVS, 16 MB, QSPI: a signed A/B image pair and a data partition for assets |
| upstream | one USB-C receptacle, USB 2.0 only, full speed, on the processor's hardware controller: the device role, the console, and the board's entire power supply |
| downstream | four full-size type-A receptacles behind a USB2514B four-port hub, driven at full speed by a PIO host on GPIO 8 and 9 |
| port power | one TPS2065C switch per port, enabled by the hub, 1 A limit, fault reported back to the hub; the hub turns ports on and off through ordinary port-power requests |
| protection | a USBLC6-2SC6 array on the upstream pair and on each downstream pair |
| input protection | a resettable 2.6 A polymer fuse in series with VBUS at the receptacle, 5 A trip |
| 3.3 V rail | AP63203 fixed-output buck, 2 A capable, from the 5 V input |
| core rail | the processor's own switching regulator, 3.3 µH inductor and the vendor's capacitor values |
| source sensing | both CC pins terminated with 5.1 kΩ and read by the ADC on GPIO 26 and 27, so firmware knows what the source advertises |
| controls | three tactile buttons on the back edge: BOOT (pulls the flash's chip select), RESET, USER (GPIO 16, internal pull-up) |
| indicators | two 0603 LEDs on GPIO 14 and 15, yellow-green and red, 330 Ω, about 4 mA each |
| debug | a 5-pin 2.54 mm SWD and UART header, not fitted; eight 1 mm test pads, bare: both USB pairs, the three rails and ground |
| clocks | 12 MHz crystal for the processor with 15 pF loads, 24 MHz for the hub with 18 pF, both 10 pF parts |

### Power

The board takes everything from the USB-C connector's VBUS. There is no other input and no
battery, so VBUS is the 5 V rail, through the fuse, and the ports are fed from it directly
through their switches: a port delivers what the source and the cable deliver, not a regulated
5 V.

| | |
| --- | --- |
| input | 5 V nominal on VBUS, 3 A at most through a USB-C to USB-C cable; the firmware reads CC to learn 0.5, 1.5 or 3 A |
| board's own draw | about 175 mA typical, 250 mA peak from 5 V, for the 3.3 V rail at about 90 % conversion |
| per port | 500 mA by the USB budget; the switch limits at 1 A by constant current, with reverse blocking and output discharge |
| worst case | 2.25 A at 5 V: four ports at the full budget plus the board |
| 3.3 V rail | 240 mA typical, 335 mA peak: processor, flash, hub and the small parts |
| core rail | from the processor's internal regulator; not a board rail |

### GPIO

| GPIO | net | |
| --- | --- | --- |
| 0, 1 | `CONSOLE_TX`, `CONSOLE_RX` | UART0, on the debug header |
| 7 | `HUB_RESET` | |
| 8, 9 | `PIO_D+`, `PIO_D-` | the PIO host pair to the hub, with 27 Ω in series and pull-downs |
| 14, 15 | `LED_1`, `LED_2` | |
| 16 | `BUTTON` | USER, to ground, internal pull-up |
| 18 | `HUB_VBUS_DET` | tells the hub its upstream is live; the way to force a re-enumeration |
| 26, 27 | `CC1_SENSE`, `CC2_SENSE` | ADC inputs through 10 kΩ and 100 nF |
| 2–6, 10–13, 17, 19–25, 28, 29 | — | free; 4 and 5 are I²C0's default pair, 19 to 25 are seven in a row, 28 and 29 reach the ADC |

Dedicated pins take the rest: QSPI to the flash, the hardware USB pair to the receptacle, SWD to
the header, XIN and XOUT to the crystal, RUN to the reset button.

### Mechanical

| | |
| --- | --- |
| outline | 100 × 49.78 mm, 1.5 mm corner radius, 1.6 mm thick |
| ports | four type-A receptacles on the front edge at 25.4 mm centres, overhanging the edge by 3.7 mm so a panel can sit flush against their faces |
| upstream | the USB-C receptacle on the left edge, facing left |
| buttons | along the back edge, right of centre; the USER button is the one an owner should reach |
| mounting | four plated M3 holes bonded to ground: two in the back corners at (4.0, 4.0) and (96.2, 3.8), two between the sockets at (24.6, 49.5) and (75.4, 49.5), measured from the board's top left |
| parts | all on the front face, 96 placed; the back face carries copper and mask only |
| silkscreen | the application's graphic and `CROSSFIRE Rev 0.1` on the front, right of the hub |

### Fabrication

| | |
| --- | --- |
| layers | four: front copper, ground plane, power plane (split between 5 V and 3.3 V), back copper |
| copper | 1 oz outer, 0.5 oz inner |
| finish | ENIG |
| track and gap | 0.15 mm minimum, 0.2 mm in use |
| vias | 0.6 mm on 0.3 mm holes, tented, none in pads |
| holes | 0.3 mm vias; 0.6 × 1.2 and 0.6 × 1.7 mm slots for the receptacle's shell; 0.65 mm pegs; 0.95 mm socket pins; 1.0 mm header pins; 2.3 mm socket shells; 3.2 mm mounting |
| impedance | none controlled: every link is full speed, and the pairs are routed as pairs over unbroken ground |
| mask | openings as the footprints draw them, at the copper's size, with the fabricator's own expansion; the receptacle's shell slots are the exception noted above |

## What is still to prove on these boards

The design's open questions are listed in `../../README.md` under "To prove on the prototype":
that 240 MHz holds at temperature on stock core voltage, what the CC readings are on real sources,
what the ports do at the bottom of the input range, and both crystals' drive level and start-up.
Nothing in this folder answers them; it records what was built so that the answers can be pinned
to it.
