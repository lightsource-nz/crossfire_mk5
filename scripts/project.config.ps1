# Per-project defaults for crossfire, read by the framework's shared light-*.ps1 layer.
#
# Three tangible boards, one application: a Pico 2 wearing the Pico-OLED-1.3 (the rig, with the
# OLED as its status display), the stock RP2040 Pico, and the production board, crossfire
# revision 0.1. The Picos put the native USB port in its host role and take their console over
# the debug probe's UART; the production board puts it in its device role, with the console on
# it as well as on the UART. All three flash and debug over SWD. A fourth, host tree runs the
# application's tests under ctest.
@{
        Name = 'crossfire'

        Trees = @{
                'conf-crossfire-host'       = 'build-host'
                'conf-crossfire-pico2-debug' = 'build-crossfire-pico2'
                #   the same targets built the way they ship. Reached with `-Preset`, since the
                # CMake target is the same either way -- what changes is the tree it is built in
                'conf-crossfire-pico2-release' = 'build-crossfire-pico2-release'
                'conf-crossfire-pico2w-debug' = 'build-crossfire-pico2w'
                'conf-crossfire-pico-debug'  = 'build-crossfire-pico'
                'conf-crossfire-v01-debug'   = 'build-crossfire-v01'
        }

        #   Bootloader names the target whose image carries this board's flash map -- the one
        # thing the script layer cannot work out for itself. With it, writing the board means the
        # bootloader, the application in the slot the map gives it and the assets in theirs, each
        # at an address read back out of the built bootloader rather than restated here.
        #   The RP2040 board has no such facility: its image is its whole flash.
        Targets = @{
                'crossfire_pico2' = @{ Preset = 'conf-crossfire-pico2-debug'; Flash = 'swd'; Bootloader = 'crossfire_bootloader' }
                'crossfire_pico'  = @{ Preset = 'conf-crossfire-pico-debug'; Flash = 'swd' }
                #   the production board carries the same flash map as the Pico 2, so it is
                # written the same way: bootloader, application in its slot, over SWD on the
                # debug header
                'crossfire_v01'   = @{ Preset = 'conf-crossfire-v01-debug'; Flash = 'swd'; Bootloader = 'crossfire_bootloader' }
        }

        #   the OpenOCD configurations belong to the framework, which is where the chip's
        # debug behaviour is understood; pointing at them rather than copying keeps this
        # project from drifting out of step with a fix made there
        Debug = @{
                'conf-crossfire-pico2-debug' = @{
                        Config = '../light_mk5/openocd-rp2350.cfg'
                        Svd    = '../../pico-sdk/src/rp2350/hardware_regs/RP2350.svd'
                        Chip   = 'RP235x'
                }
                #   the release tree is the same board and the same chip; what differs is how the
                # code was compiled, which the debugger does not care about. It is written out
                # rather than shared because this table is keyed by preset, and a release image
                # is flashed and inspected exactly as often as a debug one
                'conf-crossfire-pico2-release' = @{
                        Config = '../light_mk5/openocd-rp2350.cfg'
                        Svd    = '../../pico-sdk/src/rp2350/hardware_regs/RP2350.svd'
                        Chip   = 'RP235x'
                }
                'conf-crossfire-pico2w-debug' = @{
                        Config = '../light_mk5/openocd-rp2350.cfg'
                        Svd    = '../../pico-sdk/src/rp2350/hardware_regs/RP2350.svd'
                        Chip   = 'RP235x'
                }
                'conf-crossfire-pico-debug' = @{
                        Config = '../light_mk5/openocd-rp2040.cfg'
                        Svd    = '../../pico-sdk/src/rp2040/hardware_regs/RP2040.svd'
                        Chip   = 'RP2040'
                }
                'conf-crossfire-v01-debug' = @{
                        Config = '../light_mk5/openocd-rp2350.cfg'
                        Svd    = '../../pico-sdk/src/rp2350/hardware_regs/RP2350.svd'
                        Chip   = 'RP235x'
                }
        }

        #   one ctest, which is `cargo test` over the portable workspace -- the application's
        # own tests, reached the same way every other project's are
        Test = @{
                Preset = 'conf-crossfire-host'
                Ctest  = $true
        }

        DefaultTarget = 'crossfire_pico2'
}
