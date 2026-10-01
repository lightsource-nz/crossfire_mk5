# crossfire hardware

The production board: an RP2350 hosting four USB instruments through a hub, powered from USB-C over
Power Delivery, with an optional radio and an optional battery.

**Status: prototype, nothing built.** Every number below is a budget or a datasheet figure, not a
measurement. A predecessor design exists elsewhere as a sketch of an earlier vision; it was never
manufactured or validated, so nothing here is inherited from it — individual parts are reused only
where they stand up on their own.

## What the board has to do

- Host four USB-MIDI instruments at once, on four type-A receptacles, each supplied with 5 V.
- Keep the firmware that already exists: the USB device role and its console, the signed
  A/B image pair with probation, and the radio update path.
- Present itself to a computer on the upstream connector *while* hosting the instruments, so the two
  roles live on separate interfaces.
- Take its power from an upstream USB-C connector, separate from the one a computer plugs into.
- Optionally carry a radio for over-the-air updates and for a client app to control it.
- Optionally run from a LiPo cell for at least an hour under load.

## Three decisions shape everything else

### 1. Two USB roles on two interfaces

The RP2350 has **one** USB controller, and the board needs two roles at once: host for the four
instruments, device for the computer on the upstream connector. The roles are kept apart on separate
interfaces — the hardware controller takes one, and a full-speed port built in PIO takes the other.
Nothing is multiplexed and nothing switches role, so a computer and the instruments are live at the
same time.

**The hardware controller faces outward as the device, and PIO hosts the hub.** The deciding
argument is recovery, and it beats the alternative on its own terms:

- **The boot ROM drives the hardware controller.** Putting that pair on the upstream connector means
  BOOTSEL and USB DFU work on the connector the product actually has. Recovery needs no debug probe,
  no case opened, nobody on the bench.
- **It makes the risky new thing the recoverable one.** The host side is what has to be rebuilt, and
  if that goes wrong the board can still be reflashed over its own port. The other way round, a rough
  device port takes the console and DFU with it and leaves only SWD.
- **Bring-up starts working rather than blocked.** The board is programmable and has a console over
  its own connector before any of the new firmware exists, so the host side can be brought up
  incrementally against a board that already talks.

What it costs, plainly: **the host role moves onto a new transport.** The existing host stack sits
behind a host-controller seam, so the stack, the hub handling and the MIDI layer above it are kept —
but the packet-level transport underneath becomes a full-speed USB host built in PIO, which does not
exist in this tree. That is the single largest piece of new firmware this board asks for, and it is
the harder of the two roles to build: it has to generate frames, schedule transactions across four
devices and a hub, and handle retries.

The device side, by contrast, is already proven on hardware — the console rides it today. The one
addition there is the computer-facing MIDI interface, which is a device class on a working stack.

**The upstream connector is unambiguous.** It sinks power and acts as a device, both, always — so
the CC lines mean one thing, there is no role to choose, and the charger-versus-computer guess that a
multiplexed design would have needed is gone. Electrically it is also the simpler end: the hardware
controller's pins carry their own impedance matching and pull-ups, so the pair needs only an ESD
array and the receptacle's two data pairs tied together, so a cable can go in either way up.

**The PIO port sets the system clock, and it sets it upward: 240 MHz.** Full-speed bit timing off PIO
wants a system clock that is an integer multiple of 12 MHz, so the state-machine divider is exact and
the bit period carries no fractional jitter. The stock 150 MHz is not such a multiple. Going *down* to
120 MHz would satisfy it at the cost of a fifth of the loop; going up to 240 MHz satisfies it and buys
the loop instead, and four things line up on that number:

- **12 MHz × 20.** Twenty system cycles per bit, and an exact divide by two if the state machine wants
  to run at 120 MHz. Either way the divider is an integer.
- **Twenty cycles a bit is a generous instruction budget** for the hardest software on the board. Ten
  would have been tight. The clock that makes the timing legal also makes the program easier to write.
- **It is already proven on this silicon.** A board in this codebase runs RP2350 at 240 MHz today,
  from the crystal through a 1440 MHz VCO and a divide by six, at stock core voltage.
- **The flash stays at its fastest legal clock.** A 133 MHz part divided by two off 240 MHz is
  120 MHz. Higher multiples are worse here, not better: 288 MHz would need a divide by three and drop
  the flash to 96 MHz.

So the constraint that looked like a tax is a 1.6× clock increase, and the forwarding loop gets
faster rather than slower. The device port is unaffected either way — its 48 MHz comes from the other
PLL.

**It is an overclock, though, and that is the part to weigh.** 240 MHz is above the datasheet's
150 MHz. It is proven on the boards in hand; it is not characterised across a production population
or a temperature range, and a product ships into both. What to establish before committing: whether
stock core voltage still holds at temperature, and what the fallback is if it does not — 192 MHz
(12 × 16) is the next multiple down that still beats stock, and 120 MHz is the floor that always
works.

The PIO port is the host end, so it carries 27 Ω series resistors on the pair and host-side
pull-downs; the 1.5 kΩ pull-up belongs to the hub, which asserts it on its own upstream port. It runs
entirely on the board, from GPIOs to the hub, and never reaches a connector or a cable — which is the
kindest possible first home for a software-timed USB port.

One consequence of hosting on-board: **no host supplies VBUS to the hub's upstream port**, so the hub
is self-powered, and its upstream-detect input is driven by the MCU. The datasheet is explicit about
this case: a *detachable* hub divides that pin down from the upstream VBUS, but a self-powered hub
with a permanently attached host takes it "from a dedicated host control output, or the 3.3 V domain
that powers the host" — so a GPIO, which costs one pin and hands the firmware the ability to hold
the hub down until it is ready and to force a re-enumeration by toggling it. Without that pin
asserted the hub never believes it is connected.

### 2. Two upstream connectors, one for power and one for data

Power and data arrive on **separate USB-C receptacles**. One connector would force a choice: a port
negotiating 9 V off a charger is not a port a computer is plugged into, so whichever you wanted, you
gave up the other. Two connectors means a 27 W brick and a computer at once, and neither constrains
the other.

| | receptacle | carries |
| --- | --- | --- |
| power | type-C, power only, 6-pin | CC to the PD sink, VBUS to the charger — the designed power input |
| data | type-C, USB 2.0, 16-pin | D+/D− to the hardware controller, Rd on both CC pins, ESD, VBUS sensed |

Splitting them also cleans up what each one means. The power port negotiates without caring what a
computer would think of the result. The data port is only ever a data link — BOOTSEL, DFU, the
console and the MIDI interface all live on a connector that does nothing else.

**The data port still contributes a little power**, and it has to. Without it, a board with no
battery and no charger cannot be reflashed from a laptop, which is exactly the situation a
manufacturing line and a returned unit are in. Its contribution is capped at the USB default: about
2.5 W, which runs the MCU, hub and radio and trickles the cell, and does not pretend to run the
ports. That is not a limitation worth fighting — a laptop was never going to supply the 10 W four
ports can ask for.

> **The reverse blocking has to be automatic, not timed by firmware.** The moment the power port
> negotiates 9 V, that voltage must not appear on a laptop's VBUS, and there is no software fast
> enough to be trusted with it. So the data port's contribution goes through a reverse-blocking
> element that the higher voltage shuts off by itself.

**Neither way of plugging it in wrongly does harm**, which matters because two identical receptacles
on one edge guarantee someone will. A charger in the data port presents as an ordinary 5 V source and
is used as one. A computer in the power port supplies 5 V and simply has no data pins to reach. Both
cases degrade; neither damages.

### 3. What "under load" means

Four ports at full USB 2.0 spec is 4 × 500 mA = 10 W, and an hour of that needs a cell around
4500 mAh. Class-compliant MIDI instruments draw 100–250 mA. Sizing the battery for a load no MIDI
instrument presents buys a much larger cell for nothing.

**Taken as:** the ports run to **full spec on external power**, and to a declared **250 mA each on
battery**. The power manager already exists and is portable, so the budget is policy rather than
silicon — the current limits in the port switches protect the hardware, and the policy decides what
is offered. This is the one assumption here that most wants confirming.

## Block structure

```mermaid
graph LR
  CP[USB-C, power only] -->|VBUS| PD[PD sink]
  CD[USB-C, data] <-->|data, hardware device| MCU
  CD -->|5 V, capped and reverse-blocked| CHG
  PD -->|9 V| CHG[charger and power path]
  BAT[(1S LiPo)] <--> CHG
  CHG -->|VSYS| B5[boost 5 V]
  CHG -->|VSYS| B33[buck-boost 3.3 V]
  B33 --> MCU[RP2350A and flash]
  B33 --> HUB[4-port hub]
  B33 --> RF[radio]
  MCU <-->|data, PIO host| HUB
  HUB <-->|data| P[4 port switches and type-A]
  B5 -->|5 V| P
  HUB -.->|port power, overcurrent| P
  MCU <-.->|I2C| PD
  MCU <-.->|I2C| CHG
  MCU <-.->|SPI| RF
```

*Solid lines carry power or USB data; dashed lines are control.*

## Power

### Rails

| rail | source | sized for | why |
| --- | --- | --- | --- |
| VSYS | the charger's system node, 3.0–4.4 V | 4 A | the battery when unplugged, held up by the charger when not |
| 5 V (ports) | boost from VSYS | 2 A | **always the same converter**, so a port sees the same 5 V plugged in or not |
| 3.3 V | buck-boost from VSYS | 1 A | VSYS falls below 3.3 V on a flat cell, so a plain buck could not hold it |
| 1.1 V (core) | the MCU's own switching regulator | — | not a board converter, but it needs board parts: an inductor on `VREG_LX` and a feedback network, which is why it is a rail and not just decoupling |

The core rail catches people out, so it is worth stating: this MCU regulates its own core with a
*switching* regulator, not a linear one. It brings out `VREG_VIN`, `VREG_LX` and `VREG_FB`, so the
schematic carries an inductor and the rail it makes feeds the `DVDD` pins. The analogue supply for
the ADC wants its own filter off 3.3 V as well, which is worth keeping even with nothing presently
reading through it.

Port power comes from VSYS rather than straight off VBUS. It costs a conversion when plugged in
(about 90%), and it buys two things: a 5 V rail that does not change character when the cable is
pulled, and the freedom to negotiate 9 V upstream instead of being held to the 15 W that 5 V/3 A
allows. The cost-down that would have removed the boost — ports fed from VBUS, with the charger's
OTG output back-driving it on battery — died with the single upstream connector: VBUS is now a
power-only receptacle, and back-driving it powers nothing. The separate boost is settled.

### Budget

**3.3 V rail**

| | typical | peak |
| --- | --- | --- |
| RP2350A, both cores at 240 MHz, USB active | 110 mA | 150 mA |
| QSPI flash, executing in place | 15 mA | 25 mA |
| hub | 90 mA | 120 mA |
| radio, connected idle / transmitting | 60 mA | 350 mA |
| indicators, crystals, pull-ups | 25 mA | 40 mA |
| **total** | **300 mA** (0.99 W) | **685 mA** (2.3 W) |

**5 V rail**

| | current | power |
| --- | --- | --- |
| full USB 2.0 spec, 4 × 500 mA | 2.00 A | 10.0 W |
| declared battery budget, 4 × 250 mA | 1.00 A | 5.0 W |
| four typical MIDI instruments | 0.4–0.8 A | 2–4 W |

**Upstream**

Worst case on external power is 10.0 W of ports and 2.3 W of device, which is 13.6 W at VSYS after
conversion and about 14.8 W at the PD rail. Charging a 3000 mAh cell at 1.5 A adds 6 W.

> 5 V/3 A is 15 W and cannot do both. **Request 9 V at 3 A (27 W)**; accept 9 V/2 A with the charge
> current trimmed; fall back to 5 V/3 A with the ports held to the battery budget.

**Battery**

At the declared budget the board draws 6.7 W at VSYS. An hour of that is 6.7 Wh; a 1S cell at 3.7 V
nominal carries 1800 mAh of it, and roughly 90% is usable down to cutoff before any allowance for
age.

> **Specify a 3000 mAh 1S LiPo (11.1 Wh), rated 2 C continuous or better.** That is about 77 minutes
> at the declared budget and about 42 minutes with every port drawing its full 500 mA. Peak
> discharge at full spec is around 3.5 A, which is why the C rating is called out.

## Parts

Stock KiCad symbols exist for everything marked with one; the rest need drawing.

| block | part | symbol | why |
| --- | --- | --- | --- |
| MCU | RP2350A, QFN-60 | `MCU_RaspberryPi:RP2350A` | 30 GPIO is enough (see below); RP2350B if a display is added |
| flash | W25Q128JVS, 16 MB | `Memory_Flash:W25Q128JVS` | the A/B image pair plus the asset partition; 2 MB would not hold it, and its 133 MHz rating covers the 120 MHz the divider lands on |
| hub | USB2514B | `Interface_USB:USB2514B_Bi` | four downstream ports, per-port power and overcurrent pins, no external EEPROM needed |
| port switch ×4 | TPS2065C | `Power_Management:TPS2065CDBV` | 1 A threshold, **active-high enable, open-drain fault** — what the hub needs at both ends. It limits by going into constant current rather than latching off, and adds reverse blocking and output discharge, both of which a port wants. Its 0.5 A sibling is the same trap as before: that is the budget itself, with nothing for inrush |
| port data ESD ×4 | 2-channel array, low capacitance | — | needed because the switch above protects power only. The TPD3S0x4 would have covered both, but it has no fault output, so it cannot tell the hub anything |
| PD sink | HUSB238 | `Interface_USB:HUSB238_xxxDD` | already proven on hardware in this codebase, and it reports which profile was accepted |
| charger / PMIC | BQ25798 | `Battery_Management:BQ25798` | buck-boost, takes 9 V in directly, 1S charge, and an I²C ADC the power manager can read |
| 5 V boost | TPS61089 | `Regulator_Switching:TPS61089` | 5 A switch, comfortably 2 A out at 5 V from a 1S cell |
| 3.3 V | TPS63060 | `Regulator_Switching:TPS63060` | buck-boost, so 3.3 V survives a flat cell |
| ship switch | DMNH4015SSD | `Transistor_FET:Q_Dual_NMOS_S1G1S2G2D2D2D1D1` | the same package again, both halves in parallel: ~10 mΩ at the 4.5 V available, so 35 mV and 0.12 W at the 3.5 A peak, and 18 mV at the declared budget. 30 nC against a 100 nA charge pump at half duty is about 600 ms to close, which is nothing for a power-path switch |
| input switch ×2 | DMNH4015SSD | `Transistor_FET:Q_Dual_NMOS_S1G1S2G2D2D2D1D1` | one dual N-channel package per upstream port — the two sources tied together, the two gates tied to that port's driver. 40 V, 20 mΩ at the 4.5 V the charger's charge pump provides, 7.5 A, ±20 V gate, AEC-Q101. Pin 1 S1, 2 G1, 3 S2, 4 G2, 5–6 D2, 7–8 D1 |
| upstream data ESD | 2-channel array, low capacitance | — | the downstream pairs get theirs from the port switch; the upstream pair has no such part in front of it |
| power connector | USB-C, power only, 6-pin | `Connector:USB_C_Receptacle_PowerOnly_6P` | no data pins to mis-wire, and nothing on it a computer would want |
| data connector | USB-C 2.0, 16-pin | `Connector:USB_C_Receptacle_USB2.0_16P` | |
| port connectors ×4 | type-A | `Connector:USB_A` | |
| radio | Radio Module 2 (RMC20452T) | `crossfire:RM2`, drawn here | the same CYW43439 the firmware is already proven against, as a module rather than a bare chip so the radio arrives certified. Chosen over the other module of the same silicon because this one presents the **serial** host interface the proven driver speaks; the alternative offers only the wider bus. 21 castellated pads, −30 °C to +70 °C |
| radio bus resistors | 470 Ω, 10 kΩ | `Device:R_Small` | not optional: one host pin serves three module pads, and these are what let it |

Two notes on the hub. Its upstream link runs at full speed, because the RP2350's controller is
full-speed only — so the whole tree is full speed and instruments that could do high speed fall
back, which costs nothing, as USB-MIDI 1.0 is a full-speed class. And the hub drives the port
switches itself: `PRTPWR[1:4]` to each switch's enable, each switch's fault back to `OCS_N[1:4]`. Port
power then goes on and off through ordinary hub requests that any host stack already makes,
overcurrent arrives as a standard port status change, and eight GPIOs stay free.

### Passives

A value on its own is not a specification, so every capacitor now carries a **voltage rating and a
dielectric**, chosen from the net it sits on rather than from a house default. The rule is at least
twice the working voltage, rounded to a standard rating, and never below what a socket the user can
reach is able to present.

| what it decouples | rating | dielectric |
| --- | --- | --- |
| the core rail, 3.3 V, the system rail, the cell, the bootstraps and the internal filter nodes | 16 V | X7R |
| the 5 V rail, each port's own output, the charger's input and the node beside it | 25 V | X7R |
| the connector side — the sink's own supply, and the data port's | **50 V** | X7R |
| crystal loads, the feedback tap and the compensation network | 50 V / 16 V | **C0G** |

Two of those rows are decisions rather than arithmetic.

**The connector-side parts are rated for abuse, because nothing on the board protects them.** Both
input switches sit *downstream* of them, so the sink's own supply decoupling and the data port's
are exposed to whatever a hostile charger produces, with no switch between. They are rated for a
fault rather than for the 9 V and 5 V they are supposed to see.

**The charger's own input bank is rated 25 V on a condition, and the condition is firmware.** The
input over-voltage trip defaults to **26 V**, which is above a 25 V part — so rating the bank for
the working voltage is only safe if that trip is moved down. The part offers 7 V, 12 V, 22 V and
26 V, and **12 V is the right one for this board**: the sink never asks for more than 9 V, so 12 V
clears the profile with room and bounds what the bank can ever see. This is a board decision that
creates a firmware requirement, which is exactly the sort that gets lost, so it is written here as
well as in the firmware's own notes.

**Resistors carry a tolerance, and it is 1% only where the value is read.** Nineteen of the
thirty-eight are: a reference a part trims itself against, a divider ratio, a termination that
states what this end of a cable is, a value that programs a threshold at power-on. The other
nineteen are pull-ups, pull-downs, straps and series parts, where 5% is honest and 1% is a cost
with nothing on the other side of it. Power is not a field: the largest dissipation anywhere on
the board is a few milliwatts in an indicator's series resistor, so the smallest standard part
covers every one of them.

**Three values were arithmetic that had never been rounded to a part you can buy.** Two are the
charger vendor's own figures for the temperature network -- 5.24 k and 30.31 k, neither of which
exists -- and one was ours for the boost's current limit. They become 5.23 k, 30.1 k and 174 k,
and the cost of each is stated rather than waved at: 0.2% and 0.7% on the temperature thresholds,
which is a fraction of a degree, and about 2% on the current limit.

That last one is worth stating properly, because the shorthand for it invites a misreading. The
limit is **not** set *at* the worst-case peak. The boost's switch has to pass about 4.25 A at the
bottom of the cell with every port drawing its full share, so a limit there would trip in normal
service; it is set at about **5.9 A**, which clears that peak by nearly 40% and still leaves the
converter protecting itself. The two resistance figures the vendor publishes against measured
limits are what fix the relationship, and they also fix its direction: more resistance, less
current.

**The four inductors are specified by what they have to carry, not by their value alone**, and two
of the four had their value fixed for them by a decision already taken.

| | value | has to stay unsaturated to | why that number |
| --- | --- | --- | --- |
| core regulator | 3.3 uH +/-20% | **1.5 A** | the processor's own datasheet says so, along with a 250 mohm ceiling on resistance |
| charger | 1.0 uH | **7 A** | the input can deliver 27 W and the system sits near 4 V, so the output side carries about 6.3 A, plus half the ripple |
| 3.3 V buck-boost | 1.0 uH | **3 A** | above the converter's own 2.0-2.5 A limit, so the part protects itself before the core does |
| 5 V boost | 2.2 uH | **7 A** | above the 5.9 A the current-limit resistor sets, for the same reason |

**The charger's inductor value is not a choice.** Its datasheet is explicit that 1.5 MHz works only
with 1 uH and 750 kHz only with 2.2 uH -- and the frequency was already fixed, at power-on, by the
resistor that also sets the cell count. One resistor therefore picks the inductor, and the two have
to be changed together or not at all.

**The core regulator's inductor has a requirement that is easy to lose**: it must be fully shielded
and **marked for polarity**, and oriented the way the vendor's layout shows. Their own note is that
stray field from the inductor couples into the regulator's control loop and moves the output even
with nominally shielded parts. That is a placement constraint arriving from a component datasheet,
so it is recorded here where layout will look for it.

**The crystals are specified by load, not just frequency**, because the load is what decides
whether they oscillate on frequency at all. Both work out at **10 pF**, and both vendors name the
arithmetic rather than leaving it to be guessed.

The processor's is simply stated: its vendor recommends a specific part and publishes its
parameters, 10 pF load and +/-30 ppm either side. The hub's has to be derived, and the derivation
is the part worth writing down, because the obvious way to do it is wrong. Stray capacitance sits
in parallel with **each** loading capacitor, not with the two of them in series -- so the load is
not half the pair plus the stray, it is half of *(the pair plus the stray)*. With the 18 pF the
hub's own documents call typical and a few picofarads of board stray, that is about **10.5 pF**,
and the nearest standard part is 10 pF. The naive arithmetic gives 12 pF, which is a different
part. Both of the vendor's two formulations -- the older datasheet's and the newer checklist's --
agree on this once they are read carefully.

Its frequency allowance is **+/-350 ppm**, and that number is a trap in the other direction: it is
the *total* the hub can tolerate, so a part sold as +/-350 ppm would spend the entire budget before
temperature and ageing are counted. A commodity +/-50 ppm part meets it several times over.

**There is no feedback resistor across either crystal, and none is wanted.** The hub's datasheet
figure draws a 1 Mohm across the pins without saying which side of the package it is on; its
design checklist, which exists to list what the board has to provide, asks only for the two
loading capacitors. The same checklist confirms two things already drawn: the 12.0 kohm 1%
reference to ground, and that the reset line may be held by the processor rather than pulled up,
which is what the pull-down and the GPIO on it amount to.

### Mechanical, and the connectors

**The enclosure is drawn around the board, not the board around an enclosure**, and the first one
runs bare on a bench. That ordering decides more than it sounds like: the board may take whatever
outline suits it, but it has to be usable with nothing around it, so parts that are pressed or
looked at face upwards rather than sideways, and anything that takes a plug's insertion force goes
through the board rather than resting on pads.

**One constraint fixes the board's width, and it is the port pitch.** Four type-A receptacles at
**25.4 mm between centres** span 76.2 mm centre to centre. The widest stock footprint for a
full-size horizontal receptacle occupies 22.8 mm and the narrowest 15.4 mm, so every one of them
fits the pitch -- the narrow end leaving about 10 mm of clear board between neighbours, which is
where each port's switch, its protection array and its bulk capacitor have to live. Taking the
narrow one, the four ports occupy 91.6 mm of courtyard, and with a few millimetres of edge each
side **the port edge cannot be shorter than about 100 mm**. That is now a fixed dimension of the
product, not an outcome of layout.

It is worth recording *why* 25.4 mm rather than something tighter, because it looks generous
against a 13 mm receptacle and invites being trimmed later. The number is not about the
receptacle, it is about the **plug**: a moulded type-A plug is commonly 16 to 20 mm across, so a
narrower pitch yields a hub whose four ports cannot all be used at once. That is the failure this
product exists to avoid.

| | part | why |
| --- | --- | --- |
| downstream ports x4 | full-size type-A, horizontal, **through-hole** | the plug's force goes into the board. Every stock option is through-hole, which settles it |
| upstream data | 16-pin type-C, surface pads with **through-hole shell tabs** | the signal pads are fine surface mount; the shell takes the strain through the board |
| upstream power | **6-pin** type-C, same mounting | see below |
| indicators, buttons | edge-mounted, facing **up** | pressable and visible on a bare board; an enclosure then needs a hole rather than a plunger |
| debug | 5-pin header on 2.54 mm | bench-first means a header that any probe's flying leads reach |
| cell | two-way, 2.0 mm pitch | a two-wire pack, which is what is actually sold; the sensor it does not carry is on the board |

**The power port is physically a different connector from the data port, and that is worth having.**
A 6-pin power-only receptacle is a stocked part with a stock footprint, so taking it costs nothing
over using a 16-pin part with its data pins unconnected -- and it means the power port **cannot**
carry data, in copper rather than by labelling. The two receptacles were always going to be told
apart by what is printed beside them; this makes the printing a convenience rather than the only
thing standing between a user and a surprise.

**The cell is a two-wire pack on a 2.0 mm connector, so the thermistor moves onto the board.** The
charger's temperature network is unchanged -- the same two resistors, computed by its vendor for a
10 kohm sensor -- and the thermistor now sits across the lower leg exactly where the pack's one
would have connected. It must be a **10 kohm B3435** part, because that is the sensor those two
resistors were calculated against; a different beta moves both ends of the window.

**What this protects against changes, and the change is worth being honest about.** A thermistor
inside the pack watches the cell. One on the board watches the board. It can no longer catch a cell
heating up on its own -- the failure people picture when they think of battery thermal protection --
and what it does instead is refuse to charge when the surroundings are too cold or too hot for
charging to be safe. That is not nothing: charging a lithium cell below freezing plates lithium and
damages it permanently, and ambient is a fair proxy for a resting cell's temperature. But it is a
different guarantee, and the specification should say which one the product makes.

**It also turns a component into a placement requirement.** The sensor has to sit against the cell
and away from everything that gets warm -- the charger, the two converters and their inductors. Put
it near a heat source and it reads high and refuses to charge a perfectly good cell; put it out in
open board and it reads ambient and reports nothing about the battery at all. Of the two mistakes
the first is the one that will be blamed on firmware.

### GPIO budget

Seventeen of the thirty, and the assignment is not arbitrary — the PIO pair is forced adjacent and the
rest fall out of keeping peripherals on their default pins.

| GPIO | net | why there |
| --- | --- | --- |
| 0, 1 | `CONSOLE_TX`, `CONSOLE_RX` | UART0's default pair |
| 2, 3 | — | spare |
| 4, 5 | `SDA`, `SCL` | I²C0's default pair; reaches the PD sink and the charger |
| 6 | `CHG_INT` | |
| 7 | `HUB_RESET` | |
| 8, 9 | `PIO_DP`, `PIO_DM` | **forced adjacent**, low pin first — the PIO program addresses the pair as a base and an offset |
| 10 | `RF_PWR` | |
| 11 | `RF_CS` | |
| 12, 13 | `RF_CLK`, `RF_DIO` | kept adjacent so the radio's PIO program can side-set the clock beside its data pin |
| 14, 15 | `LED_1`, `LED_2` | |
| 16 | `BUTTON` | |
| 17 | `VBUS_DET` | data-port VBUS present, so a self-powered device only attaches when a host is there |
| 18 | `HUB_VBUS_DET` | tells the hub its upstream is live; also the way to force a re-enumeration |
| 19–25 | — | spare, and still contiguous: seven in a row is a display bus |
| 26–29 | — | spare, and the only four that reach the ADC on this package |

Dedicated pins take the rest: QSPI to the flash, the hardware USB pair to the data receptacle, SWD to
the debug header, XIN/XOUT to the 12 MHz crystal, RUN to reset.

The boot button is not a GPIO. It pulls the flash's chip select low, the way the reference design
does, so it costs nothing from the budget above — and the button on GPIO 16 is a separate, ordinary
one.

## Sheets

| sheet | contents |
| --- | --- |
| `mcu` | RP2350A, flash, crystal, SWD, boot button, decoupling |
| `upstream_power` | the power-only receptacle, CC and the PD sink, VBUS out to the charger |
| `upstream_data` | the data receptacle, Rd, ESD on the hardware USB pair, VBUS sense and its capped contribution |
| `hub` | USB2514B, its 24 MHz crystal, the `RBIAS` resistor, the PIO host pair's series resistors and pull-downs, upstream detect from the MCU, and four downstream pairs |
| `port` | one port: switch, ESD array, receptacle, bulk capacitance — instanced four times |
| `power` | the ORing of the two inputs, the charger, battery connector and thermistor, the 5 V boost, the 3.3 V buck-boost |
| `radio` | the module, its bus and supply — fitted or not |

## The project as it stands

`crossfire.kicad_pro` is a KiCad 10 project holding the root sheet, seven child sheets and a project
symbol library for the parts with no stock symbol. The root carries the decomposition and the
interface: every sheet has its pins, every pin has a matching hierarchical label inside its sheet,
and every pin carries a named stub, so each net that crosses a block boundary is named and agreed.
`port.kicad_sch` is drawn once and instanced four times, with the root mapping its generic
`DP`/`DM`/`PWR_EN`/`FAULT` onto the hub's `Pn_*`.

**All seven sheets are populated.** `mcu` holds the MCU, the flash, the
core-rail inductor and its decoupling, the crystal, the analogue filter, reset and boot, the debug
and console header, the indicators and the button. `hub` holds the hub, its 24 MHz crystal, the
`RBIAS` reference, the two regulator filters, the straps that select the default configuration, the
reset and upstream-detect pull-downs, and the upstream pair's series resistors and host-side
pull-downs. Every pin of every part is on a named net. `port` holds the current-limited switch, the ESD array, the receptacle and the bulk capacitance -- drawn once and placed four times. `upstream_data` holds the data receptacle, its terminations, the protection array and the sense divider; its contribution to the rail now leaves the sheet directly, because the switch beside the charger does the blocking that used to need a diode here. `power` holds the charger with every bank the datasheet specifies, the thermistor network, the cell connector, and the two converters that make the rails. `upstream_power` holds the power receptacle, the sink that negotiates on it, and the two resistors that state the contract it asks for before any firmware is running. `radio` holds the module, the two resistors that let one host pin serve three of its pads, its decoupling, and all six of its ground pads taken to the plane.

Both are drawn the way a person would draw them: **signals are wires**, and only the rails and the
nets that leave the sheet are carried on symbols and labels. Parts sit beside the pins they serve —
the flash is placed so all six lines of its bus are single straight segments across to the MCU, and
on the hub every passive is turned side-on so it sits in line with its own pin. That last one is
what keeps a chip with fourteen left-hand pins from becoming a knot: each pin owns a row, and
nothing has to cross anything.

**A sheet placed more than once needs care that a sheet placed once does not.** `port` is drawn
once and used four times, and each placement is a real set of parts with its own references. KiCad
carries that inside the symbol: one instance path per placement, each naming the reference that
placement uses. A symbol carrying a single path — which is all a sheet used once ever needs — leaves
the other three placements with no parts in them at all. Worth knowing before drawing any other
repeated sheet, and worth checking by reading the netlist rather than the drawing: the four
placements should name twelve different parts, not three.

It can all be checked without opening the editor:

```
kicad-cli sch erc      -o erc.rpt crossfire.kicad_sch
kicad-cli sch export netlist -o crossfire.net crossfire.kicad_sch
```

The netlist is real: 138 nets, and every one of the twenty-seven unconnected pins is deliberate — the
twelve spare GPIOs the pin map names, the three hub pins the datasheet says to leave alone, the
charger's data-detect pair and its ship-mode input, the receptacle's sideband pins, and the
buck-boost's power-good output. Nothing has been left connected by accident, and nothing intended
has been missed.
Read it rather than the picture when checking this sheet; a wire that looks right and a wire that
*is* right are not the same thing, and the netlist is the one that answers.

ERC on the two drawn sheets reports only what is genuinely outstanding: `+3V3` and `GND` have no
source until the `power` sheet exists, and the hub's four over-current inputs have nothing driving
them until the `port` sheet does. The locally
generated rails, `+1V1` and the analogue supply, carry flags because a passive inductor and a
ferrite are not power sources.

The rest of the report is the six empty sheets: dangling hierarchical labels and stubs that reach no
pin. That count is the thing to watch, and it falls as each sheet is populated.

## Settled since

**The host stack's seam carries a PIO transport.** This was the first thing to establish and it
holds up. The stack is written against a `HostController` trait of eight methods — device detect,
root-port reset, control, bulk in, bulk out, and interrupt-pipe allocation — and everything above it
is generic over that trait, including the enumeration and the hub handling. It already has three
implementations, two chips and a mock, and the mock is the proof: the stack has been substituted
before. So the enumeration, the hub handling and the MIDI layer are all kept, and only the transport
underneath is new.

What that does *not* buy is a smaller job. The chip implementation is about 1,600 lines, and it
divides in two. The transfer-level half — splitting transfers into packets, reassembling them, pipe
bookkeeping — carries over in shape. The other half is register writes that ask the silicon to
generate frame markers and keep-alive, and to do CRC, bit stuffing, encoding and retries. **A PIO
transport has to supply all of that in software**, on a one-millisecond frame cadence and inside the
bus turnaround window. The seam being clean means the work is contained, not that it is small.

**The hub needs no EEPROM, and its defaults are already what this design wants.** Its configuration
register 06h powers up at 9Bh, and that value decodes to self-powered operation, over-current
sensing on a port-by-port basis, and port power switching on a port-by-port basis — the three things
this design depends on, all of them the factory default. An EEPROM or an SMBus master would only be
needed to change them, or to give the hub custom identifiers.

The same register answers the speed question. Its high-speed-disable bit is clear by default, which
means the part attaches as high- *or* full-speed, whichever the host offers; a full-speed host gets a
full-speed hub, and no configuration is required to make that happen. The bit exists to force
full-speed only, which is worth knowing about but not worth an EEPROM to set.

Two details that came out of the same reading and would have been found the hard way: the
over-current inputs carry internal pull-ups and are active low, so a switch with an open-drain fault
output connects straight to them with nothing in between; and the part wants a **12.0 kΩ ±1%
resistor from `RBIAS` to ground** to set its transceiver bias, which is easy to leave off a
schematic and not easy to diagnose afterwards.

**The 9 V request is three writes, and this codebase has already made them work.** Select the
profile by writing its index into the selection register, then write the go command; the order is
not interchangeable, because the go command acts on whatever the selection register holds at that
instant. Do not read the result back immediately — the negotiation finishes in its own time, and an
immediate read reports the *previous* contract and looks like a failure.

The trap is worth restating because it cost real time to find once already: **every register must be
written as a strictly framed two-byte transaction.** A general write path that sends the address and
the payload as two transfers separated by a repeated start is acknowledged by the part and stores
nothing — the selection register took five writes of five different values and read back zero after
every one, while every return code said success.

**The CC senses come off the design.** They were there so the firmware could learn whether the data
port's source offers more than the USB default. Asking what they needed answered the question
differently: they buy nothing, because the data port's contribution is capped by intent rather than
by what the source offers — it exists so a board with no battery and no charger can be reflashed,
not as an operating mode. Removing them takes two pins, four resistors and a clamp question off the
board. It also removes a subtler risk: any divider across CC sits in parallel with the termination
resistor that tells the source what we are, and shifting that is worse than not knowing.

**Which turns the ORing into one diode.** *(Superseded further down: the charger turns out to do
the ORing itself, so the diode gives way to a second pair of transistors. The reasoning below still
holds for why the controller came off, and it is what made the charger's own mux the obvious
replacement.)* A prioritised ORing controller with external FETs was
specified because two sources meet at the charger and 9 V must never reach the data port. But the
data port now contributes at most the USB default, and a path that small can afford a Schottky: it
blocks reverse absolutely and without being told, it costs about a fifth of a watt on a path that is
rarely used at all, and the charger's buck-boost input does not care about the drop. The power port
stays direct, because a diode at 3 A would not be affordable — **on the condition that the sink's
own output switch opens when nothing is attached**, which is the one thing left to confirm there.

The charger can then tell its two sources apart without being told either: they arrive at different
voltages, and it measures input voltage already. Setting the input current limit from that is
firmware work, not a board question.

**The port switch had to change, and the reason is a pin that is not there.** The design said the
hub drives each port's switch and reads its fault back — which is the whole argument for letting the
hub own port power. But the part chosen for it, which integrates the current-limited switch and the
data-line protection in one package, has six pins: enable, ground, in, out, and the two data lines.
**There is no fault output.** It protects itself perfectly well and tells nobody, so the hub's
over-current inputs would have sat at their pull-ups reporting that all was well, for ever.

So the port becomes two parts instead of one: a current-limited switch that does have a fault
output, and a separate ESD array for the data pair. That costs four parts across the board and it
is the right trade — the alternative is a self-powered hub that cannot report over-current, which is
not a thing to ship.

Two polarities have to match and both are available in the wrong flavour, so they are worth naming.
The enable must be **active high**, because that is what the hub's port-power outputs drive, and the
same libraries carry active-low-enable siblings that would leave every port energised exactly when
the hub asked for off. The fault must be **open-drain and active low**, which suits the hub's
inputs directly — they are pulled up internally, so nothing goes between them.

**And one thing the switch still will not do: enforce the budget.** A 1 A threshold sits where it
should — above the port's 500 mA with room for inrush, and far enough below the rail to be worth
having — but four ports in limit is 4 A against a rail built for 2. That is the normal arrangement
rather than a fault: the budget is enforced above, by the hub's port power control and the
firmware's policy, and it only arises with four misbehaving devices at once. It does make **the
rail's own limit the backstop**, so the boost has to current-limit gracefully rather than latch off.
The chosen boost sets its limit with an external resistor, which is better than inheriting one —
what it does on reaching that limit is the part still to confirm.

### What the charger wants, from its pin table

Read off the datasheet rather than inferred, because most of it could not have been. The unused
blocks turned out to be the part that needed telling, and none of it was guessable:

| pin | what it needs | why it is not obvious |
| --- | --- | --- |
| `VAC1`, `VAC2` | **to VBUS** | they are input-detect pins; with no external input FETs they watch the input directly |
| `ACDRV1`, `ACDRV2` | **to ground** | gate drives for input FETs that are not fitted |
| `ILIM_HIZ` | **to REGN** | that selects the maximum input limit and hands the budget to the register the firmware writes — exactly what this design wants, and the alternative is a divider computed from a milliohm term |
| `SDRV` | **to the ship switch's gates** | it drives that switch from a 100 nA charge pump, 5 V above the cell. The 1 nF this pin wants when no switch is fitted is therefore *not* fitted: at 100 nA it would buy nothing but delay |
| `CE` | **pulled low** | "must be pulled HIGH or LOW, do not leave floating"; low enables charging under register control |
| `BATP` | **100 Ω in series to the battery** | a sense input, not a power pin |
| `TS` | divider from `REGN`, with a 10 kΩ NTC | charging suspends when it reads out of range, so an absent thermistor is not a neutral state |
| `STAT`, `~INT`, `SDA`, `SCL` | 10 kΩ to the logic rail each | all four are open-drain or bus lines |
| `QON` | left alone, but now worth a test pad | it has an internal pull-up, and with the ship switch fitted its two jobs become real: leaving ship mode, and a power reset that does not go through firmware. Both are available over the bus as well, so this buys only the firmware-independent path — and the pin idles at 3.6–3.8 V, above what a 3.3 V input should see, so sharing the existing button is not free |
| `PROG` | **3.0 k, 1%** | it sets the power-on cell count and switching frequency together: 3.0 k selects one cell at 1.5 MHz, which is the frequency the 1.0 uH inductor belongs to |

The capacitor banks are specified rather than chosen: three at `PMID`, two at `VBUS`, five at `SYS`,
two at `BAT`, each with a 0.1 µF alongside where the datasheet asks for one, 4.7 µF at `REGN`, 47 nF
bootstraps, and a 1.0 µH inductor between the two switch nodes.

**The PD sink has no output switch of its own, and it turns out it does not need to have one.** The question was whether
it opens that switch when nothing is attached, so the data port cannot back-drive an exposed
connector. Its pinout answers it differently and better: there is no internal switch to open. The
part brings out a gate drive for an external FET, so the switch is ours to place and ours to
orient — and a FET we choose can be made to block reverse by construction rather than by trusting
someone else's internal arrangement. The condition on the power port is therefore a design task, not
an unknown.

It also explains the two spare pins beside it. The part takes a requested voltage and current as
resistor settings as well as over the bus — and on this board that is not a second way of doing the
same thing, it is the only way the board starts. **These two resistors are the contract the board
boots on**, and they are now fitted. A fixed 100 µA flows out of each pin, so a resistor to ground
states a level the part reads as a voltage and a current, and it asks for them on its own the moment
a charger is attached. Nothing else can: the rail they bring up is the one that powers the
processor, so with a flat cell there is no firmware awake to ask over the bus.

The charger meets them from the other side, which is what makes the cold start hold together. It
states that charging is enabled by default at power-on and that it will run a complete cycle with no
software involvement at all, and that when both its input and the battery are below their thresholds
it powers *itself* from whichever detect pin sees a valid source first. So the sequence from a flat
cell and a bare charger needs nothing but these two resistors and the charger's reset defaults.

**So they ask for the modest contract, not the wanted one.** They have to succeed against whatever
charger is on the end of the cable, and a cold boot draws a fraction of what a working board does.
6.04 kΩ selects 9 V and 10.5 kΩ asks for 2 A of it. The board then takes the larger contract in
firmware, by selecting a source capability by index over the bus, which is a path already proven on
hardware.

**The matching rule is what makes those the right two numbers**, and it is worth stating because it
is not the obvious one. The part forms a request from the lower of the resistor setting and its own
internal default, then walks the source's offers **from the highest voltage down**, taking the first
whose voltage is at or below the request and whose current is at or above it. Two things follow.

The voltage setting is a *ceiling*, not a target, and raising it can never lose a lower offer — so
9 V is chosen to bound what an unattended cold start is allowed to put on the charger's input, which
is what the input components either side of it are specified for, rather than to gamble on what is
available. And the current setting is a *floor* the offer has to clear, which is the opposite way
round, so asking for more narrows the field rather than widening it: at 3 A a common 18 W charger
fails the 9 V line and the scan drops through to 5 V/3 A, giving 15 W, where asking for 2 A takes
9 V/2 A and gives 18 W at a voltage the charger converts from more comfortably. The modest request
is the one that wins more power here, which is why it is not merely the cautious choice.

**There is no way for this to end with no contract.** If an offer fails on either test the part
moves to the next one down and keeps going, and every source offers 5 V, so the floor is the 5 V
case the design already accepts. If there is no power-delivery source at all it falls back to the
older charger-detection schemes instead. That was an open question against this part; it is closed.

**And the switch found a better home than either answer expected.** The charger brings out a
charge-pump drive for a back-to-back pair of transistors on its own input, raising their gates above
their common source when the input is valid, and it does the reverse blocking itself. That is the
same arrangement the open question was asking for, from a part already in the design, with the gate
drive and the input-valid detection solved rather than designed. So the switch sits beside the
charger, the power receptacle's sheet exports raw connector voltage, and the sink's own gate output
goes unused.

**But fitting that switch to only the power port would strand the data port. Taken: a pair on each
port.** The charger describes three arrangements. With no transistors at
all — what is drawn today — both detect pins tie to the charger's own input and every source is
simply wired together there, which is why the two upstream feeds currently meet at a diode. With one
pair, the switched port is selected only while the *unswitched* input is quiet: among the conditions
for closing that switch is that the charger's own input pin is below its present-threshold. So a
computer already feeding the rail through the ORing diode holds the switch open, and the charger's
own instruction for changing over is to have the host disable the source on the unswitched input and
wait for it to decay — which is not something that can be done to someone else's computer.

**With two pairs, one per port, the arbitration is the charger's and it needs no help.** It closes
whichever path presented a valid input first, without host intervention; on a tie the first port
wins, and it is nominated primary; and either path can afterwards be selected deliberately by
writing a register, with a status bit per path to read back. That is a better input stage than the
diode by every measure that matters here — no forward drop on the main supply path, reverse blocking
in both directions rather than one, a defined answer when both are attached, and the choice
available to firmware instead of fixed in copper. **It costs two more transistors and deletes the
ORing diode**, and it moves the data port's contribution from the charger's input pin to the second
detect pin. The one thing it gives up is that the charger's own inspection of the data lines is
described as applying to the first port only, so a limit for the computer-fed path is a register the
firmware writes rather than something the charger works out — which this design was doing anyway.

**The transistors themselves are chosen: one `DMNH4015SSD` per switched port.** It is a dual
N-channel in a single SO-8, which is the shape this job wants — the pair's common source becomes a
short link between two adjacent pins, and the two gates land next to each other on the way to the
driver, so a pair is one part rather than two.

The charger sets three of the four requirements and the connector sets the fourth. It drives the
gate **5 V above the common source**, so the part has to be fully on at 4.5 V, where this one is
20 mΩ — two in series carry the worst-case 3 A at about 120 mV and a third of a watt, against a
package rated for 1.4 W. The **40 V** rating is the connector's doing: the charger's own
overvoltage trip defaults to 26 V and the pair has to hold off whatever arrives until it opens, so
a 30 V part leaves around a tenth of its rating as margin on a socket the user can plug anything
into. The gate rating is ±20 V against a 5 V drive, and the part is qualified to the automotive
standard, which is free here and says something about the tails.

**The gate charge is the one figure the charger does not bound, and it turns out not to matter.**
Nothing is specified about the charge pump behind that pin — neither current nor turn-on time — so
the honest question is whether it can drive what we hang on it. Two things answer it. The
evaluation board for this charger drives four transistors of 20 nC each, which is more charge than
the 15 nC of these; and slow is the right direction for an input switch anyway, since the gate
ramp is what keeps the inrush into the charger's input capacitance gentle. A charge pump that takes
milliseconds to close this switch is doing the right thing.

**The pin numbering is read off the part's own drawing, not off the convention for its package:**

| pin | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| | S1 | G1 | S2 | G2 | D2 | D2 | D1 | D1 |

It happens to agree with the usual arrangement for a dual of this kind, but that was worth
confirming rather than assuming, and confirming it took more than reading the file: the drawing is
held in a nested form with its own fonts, so the labels and the numbers come from two different
diagrams and neither carries both. The pin-1 marker resolves it — it sits beside **S1**, at the
corner the numbering starts from, with the pins running down that side and back up the other.

So a pair is wired as: the two **gates** (2 and 4) to that port's driver; the two **sources**
(1 and 3) linked to make the common source the driver lifts from; one doubled **drain** pair to the
connector, with that port's detect pin on the same node, and the other to the charger's own input.
Each end of the pair gets two pins rather than one, which is where the current wants them.

**Both pairs are drawn, and the input stage now reads as three nets where it used to read as one.**
The two feeds arrive on their own nets and stay apart; they meet only through the switches, on the
node the input capacitors and the charger share. The detect pins moved to the connector side of
their own pair, the drivers stopped being tied to ground, and the blocking diode came out of the
data port's sheet. That also retired a long-standing complaint from the rules check — the two feeds
used to be one net wearing two names, and now they are genuinely two.

**Two faults turned up in the drawing while this was being wired, and both are worth recording.**
The diode the switch replaced was **pointing the wrong way**: its cathode faced the connector, so it
would have blocked the data port from contributing anything and conducted the charger's rail back
out to whatever was plugged in — the exact failure it was placed to prevent. Removing it settles
that, but it would have survived to a board if the switch had not displaced it. And the three input
capacitors each had a **wire bridging their own two pins**, which shorted the charger's whole input
to ground. Both were found by reading the connection list instead of the picture, which is the habit
this project keeps for exactly this reason.

**A bridged part is the one of those the rules check genuinely cannot see**, and it is worth being
exact about why, because the looser version of this claim is wrong. Bridging a part is not illegal:
it is just a net. What the check reports, if anything, is a *symptom*. When the bridge shorts two
**named** nets — as the capacitors did, tying the charger's input to ground — it reports that two
names are attached to the same items, which is a real complaint about the wrong thing and easy to
wave through as a known one. When only one side carries a name, it reports **nothing at all**:
bridging the radio's series resistor leaves the violation count unchanged and silently removes a net.
That was measured, not assumed.

**A third fault, found while specifying the passives, was the worst of them.** The 3.3 V
converter's feedback divider was not connected to its feedback pin. The divider is right -- 1 M
over 178 k against a 500 mV reference puts the output at 3.3 V -- and its midpoint wire *crossed*
the feedback line, one grid step from joining it, with no junction at the crossing. That draws
exactly like a connection. The rail it sets feeds the processor, the flash, the hub and the radio,
and with no feedback the converter has nothing telling it when to stop.

**The rules check says nothing whatever about it**, which was measured by putting the fault back:
not one violation moved. Nor would the earlier checks have caught it -- the divider's midpoint is
a net with two nodes, so it is not a stub, and nothing is bridged. What catches it is asking which
nets reach **nothing but two-terminal passives**. A divider whose output never arrived has that
shape. So does an honest series compensation network, which is why that question is reported for
reading rather than failed -- on this board it returns two lines, and the difference between them
is obvious once they are side by side.

So the check worth repeating after any hand edit is: **does any two-terminal part have its own two
pins on the same net?** `scripts/check-pcb.ps1` asks it, along with two questions the rules check
does also answer, so that one command covers the lot.

**The battery gets a switch, because without one this board has no off at all.** That is the finding,
and it is in the schematic rather than in any datasheet: the 3.3 V converter has its enable tied
straight to the system rail, and the 5 V converter's is pulled up to the 3.3 V rail. Neither is
under firmware control. So for as long as the cell is connected, the processor, the flash, the hub
and the port rail are all live, and the deepest state firmware can reach is sleep. **The ship switch
is the only thing on the board that disconnects the battery.**

It is worth being clear about what the argument is *not*. The charger's own draw in battery-only
operation is 17 µA, which on this cell is measured in years — the charger is not what flattens it.
What flattens it is everything else staying powered, and against that the charger's shutdown state
is **500 nA**. The size of the gap is the point; the exact figure on the other side of it is an
estimate of a sleeping-but-powered board and should be measured rather than believed.

**Wake is safe, which is the objection that would otherwise sink it.** Both low-power states wake on
a charger being plugged in, unconditionally. Ship mode also wakes by register or by pulling the
reset input low; the deeper shutdown wakes *only* by plug-in. There is no sequence that strands the
device.

The switch is the part already in the design for the input pairs, both halves in parallel. Two
consequences are easy to miss and both are taken: the capacitor that pin wants **when no switch is
fitted** comes out, because against a 100 nA charge pump it would only add delay; and the battery
sense resistor **stays on the cell side** of the switch, so the charger can still see the battery
while the switch is open — the same discipline that puts each input's detect pin outside its own
pair. The drains face the cell, so the body diodes block pack-to-system when the switch is open.

**The radio is a specific part now, and drawing it added components the interface did not imply.**
The module is chosen for a reason that has nothing to do with the radio and everything to do with
the driver: two pre-certified modules carry this silicon, and only one of them brings out the
**serial** host interface the proven firmware speaks — the other offers the wider bus only, which
the driver cannot use. That decided it before any electrical comparison.

**One host pin serves three of the module's pads, and the two resistors that make that work are not
optional.** The host has a single data line; the module wants its data in, its data out and its
interrupt on separate pads. The data-in pad takes the line directly. The data-out pad reaches it
through **470 Ω**, so that when the module drives and the host drives at once neither is damaged and
whichever is driving wins. The interrupt reaches it through **10 kΩ**, large enough to pull the
shared line when the module wants attention without disturbing traffic on it. Both values are the
module vendor's, and a design that read only the four-signal interface would have left them out.

Two smaller things came with it. The module enables its two radios on **separate pads**, which this
board ties together because nothing here wants one without the other. And it brings out three spare
general-purpose pins; the pin budget has thirteen spare already, so they are left unconnected rather
than used, but they are there if a later revision runs short.

**The PIO port and the radio do not contend.** Counted rather than assumed: the radio's bus takes
one state machine and eight of the thirty-two instruction slots in the first PIO block, four pins
and one DMA channel. The part has three PIO blocks of four state machines each, and sixteen DMA
channels. Giving the USB host a whole block of its own leaves a third block and most of the DMA
untouched, and any block can drive any pin, so the pin choice is not constrained by the split
either.

## Still to settle

Things this document asserts that a datasheet has to confirm before layout:

- That 240 MHz holds at temperature on stock core voltage, and what raising the core voltage would
  cost the power budget if it does not. 240 MHz is above the datasheet and proven only on the boards
  in hand, so a production population is the open question, not whether it runs.
  **And it reaches the radio**: the module's bus timing is specified against a 125 MHz system clock,
  and its vendor says the divisor has to be re-derived if the system clock changes. Ours is nearly
  double. That is firmware rather than board, but it is a consequence of the clock decision and
  belongs recorded against it rather than found at bring-up.
- The QSPI divider at 240 MHz, and that the flash part chosen is rated for the 120 MHz it lands on.
- **What the capacitor banks are actually worth once they are fitted.** The charger specifies them
  as *counts* of 10 uF parts -- three here, five there -- and that only means something if each
  part is still worth 10 uF where it sits. A ceramic loses capacitance under DC bias, and the
  banks at the input carry 9 V, which is where the loss bites hardest. The rating and the
  dielectric are settled; the **package** is not, and it is the package that decides how much
  survives. So the footprint step has to check each chosen part's own bias curve and, if the
  effective value falls short, add parts rather than assume the count was the point. This is the
  one piece of the specification that can still change the part count.
- The 5 V boost's compensation, which is carried over from the vendor's own 9 V application at the
  same input range and the same 2 A rather than computed. Everything else in that converter is
  derived -- but a compensation network is bench-verified whatever its starting values, so this is
  the right one to carry. The vendor's figure also shows a second, unlabelled capacitor at the
  compensation pin; it is not fitted here, and wants checking.

- That the 5 V boost current-limits gracefully rather than latching off, since it is the real
  backstop behind four switches that each trip well above the port budget.
- Whether the radio module's antenna keep-out can be met at the board edge. This is now a stated
  constraint rather than a guess: the module wants a metal-free region off the antenna end, a good
  ground everywhere else for the sake of its radiation pattern, and explicitly **no ground poured
  near the antenna**. It is a placement question, so it belongs to layout rather than to the
  schematic, but it is the one that decides where the module can sit.
- The module's temperature range is **−30 °C to +70 °C**, which is narrower than anything else on
  the board. It bounds the product, not the part, so it wants stating wherever the operating range
  is claimed.

The questions above the electrical detail are all answered now. A computer and the instruments are
served at once, and this design does that. The declared battery budget of 250 mA a port stands, so
the 3000 mAh cell and the hour it buys are settled rather than assumed. And the two receptacles are
told apart by labelling — on the case, and on the silkscreen beneath it — which is a reasonable
answer given that neither way of getting it wrong does any harm.

The largest piece of work this board creates is not on the board: **a full-speed USB host in PIO**,
carrying the existing host stack on a new transport. Its shape is now known — eight methods, with
the frame cadence and the bit-level protocol to supply in software — but none of it is written. It
is worth proving on a development board against the existing firmware before this design is
committed to copper, because it is the one part of the architecture with nothing behind it yet, and
because it is what holds the system clock at 240 MHz, which everything else is timed against.
