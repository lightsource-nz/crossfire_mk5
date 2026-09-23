# Per-project defaults for crossfire, read by the framework's shared light-*.ps1 layer.
#
# Two tangible boards, one application: a Pico 2 wearing the Pico-OLED-1.3 (the rig, with the
# OLED as its status display) and the stock RP2040 Pico (the product board). Both put the native
# USB port in its host role and take their console over the debug probe's UART, so both flash and
# debug over SWD. A third, host tree runs the application's tests under ctest.
@{
        Name = 'crossfire'

        Trees = @{
                'conf-crossfire-host'       = 'build-host'
                'conf-crossfire-pico2-debug' = 'build-crossfire-pico2'
                'conf-crossfire-pico-debug'  = 'build-crossfire-pico'
        }

        Targets = @{
                'crossfire_pico2' = @{ Preset = 'conf-crossfire-pico2-debug'; Flash = 'swd' }
                'crossfire_pico'  = @{ Preset = 'conf-crossfire-pico-debug'; Flash = 'swd' }
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
                'conf-crossfire-pico-debug' = @{
                        Config = '../light_mk5/openocd-rp2040.cfg'
                        Svd    = '../../pico-sdk/src/rp2040/hardware_regs/RP2040.svd'
                        Chip   = 'RP2040'
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
