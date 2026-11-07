# Reads the production board's CC lines over SWD, with the board running and its USB port free.
#
# WHY THIS EXISTS: the board learns what its supply offers from the receptacle's CC lines, and the
# one way to ask it is the console -- on that same receptacle. A cable to a computer is itself a
# supply, so the console cannot be used to test any other. This goes in through the debug header
# instead, which leaves the receptacle to the supply under test.
#
# NOTHING IS HALTED OR RESET. OpenOCD alone, with no gdb: gdb halts the core the moment it
# attaches, and a look at one register is not worth stopping the board for. The bus reads and
# writes below go past the running cores. The firmware reads the same ADC twice a second, so now
# and then a sample here lands on one of its conversions: the median ignores that, and the raw
# range will show it. For that moment the firmware can read the wrong line too, which a watching
# indicator may blink at; it corrects itself at its next read.
#
# WHAT IT SAYS: each line's raw reading across a run of conversions, the band that decodes to, and
# the indicator the firmware set from its own readings -- so a line the two disagree about is
# visible. The bands restate crates/light_app_crossfire/src/power.rs, and are checked
# against nothing: keep them in step. Millivolts assume the analogue rail is the 3.3 V the
# firmware assumes too.
#
# USAGE:  scripts/read-cc.ps1 [-Samples 16]
param(
        [int]$Samples = 16
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'light-tools.ps1')
Import-Module (Join-Path $LightScripts 'lib/LightProject.psm1') -Force
Import-Module (Join-Path $LightScripts 'lib/LightPlatform.psm1') -Force

$ADC_CS = 0x400a0000
$ADC_RESULT = 0x400a0004
$TIMERAWL = 0x400b0028
$GPIO_STATUS_RED = 0x40028070    # GPIO 14
$GPIO_STATUS_GREEN = 0x40028078  # GPIO 15
$CS_EN = 0x1
$CS_START_ONCE = 0x4
$CS_READY = 0x100
$CS_AINSEL_LSB = 12
$STATUS_OUTTOPAD = 1 -shl 9
$FULL_SCALE_MV = 3300

$config = Get-LightProjectConfig -ProjectRoot $root
$debug = $config.Debug['conf-crossfire-v01-debug']
$exe = Get-LightExeSuffix
$tools = Get-LightToolRoot
$openocd = Find-LightTool -Name "openocd$exe" -Candidates @(
        $debug.Server,
        "$tools/xpack-openocd-0.12.0-7/bin/openocd$exe",
        "$tools/xpack-openocd/bin/openocd$exe"
)
if (-not $openocd) { throw "no openocd found: not named in project.config.ps1 Debug.Server, not under '$tools', and not on PATH" }

$prefix = Split-Path (Split-Path $openocd -Parent) -Parent
$searchDir = @((Join-Path $prefix 'openocd/scripts'), (Join-Path $prefix 'share/openocd/scripts')) | Where-Object { Test-Path $_ } | Select-Object -First 1

$ocdArgs = @()
if ($searchDir) { $ocdArgs += @('-s', $searchDir) }
$ocdArgs += @('-f', (Join-Path $root $debug.Config), '-c', 'init')
$ocdArgs += @('-c', ('mdw 0x{0:x8}' -f $TIMERAWL))
foreach ($channel in 0, 1) {
        $start = ($channel -shl $CS_AINSEL_LSB) -bor $CS_START_ONCE -bor $CS_EN
        foreach ($n in 1..$Samples) {
                #   a conversion takes 2 us and a probe round trip far longer, so it has finished
                # by the time CS is read back; CS then says so, and which channel it was
                $ocdArgs += @('-c', ('mww 0x{0:x8} 0x{1:x}' -f $ADC_CS, $start))
                $ocdArgs += @('-c', ('mdw 0x{0:x8}' -f $ADC_CS))
                $ocdArgs += @('-c', ('mdw 0x{0:x8}' -f $ADC_RESULT))
        }
}
$ocdArgs += @('-c', ('mdw 0x{0:x8}' -f $GPIO_STATUS_RED), '-c', ('mdw 0x{0:x8}' -f $GPIO_STATUS_GREEN))
$ocdArgs += @('-c', ('mdw 0x{0:x8}' -f $TIMERAWL), '-c', 'shutdown')

$output = & $openocd @ocdArgs 2>&1 | ForEach-Object { "$_" }

$timer = @()
$raw = @{ 0 = @(); 1 = @() }
$red = $null
$green = $null
$channel = $null
foreach ($line in $output) {
        if ($line -notmatch '^0x([0-9a-f]{8}): ([0-9a-f]{8})') { continue }
        $address = [Convert]::ToInt64($Matches[1], 16)
        $value = [Convert]::ToInt64($Matches[2], 16)
        switch ($address) {
                $TIMERAWL { $timer += $value }
                $ADC_CS {
                        if (-not ($value -band $CS_EN)) { throw "the ADC is not enabled: the firmware has not reached its board set-up, or is not this firmware" }
                        $channel = if ($value -band $CS_READY) { ($value -shr $CS_AINSEL_LSB) -band 0xf } else { $null }
                }
                $ADC_RESULT { if ($null -ne $channel) { $raw[[int]$channel] += $value } }
                $GPIO_STATUS_RED { $red = [bool]($value -band $STATUS_OUTTOPAD) }
                $GPIO_STATUS_GREEN { $green = [bool]($value -band $STATUS_OUTTOPAD) }
        }
}

if ($raw[0].Count -ne $Samples -or $raw[1].Count -ne $Samples -or $timer.Count -ne 2) {
        $output | Select-Object -Last 12 | ForEach-Object { Write-Host $_ }
        throw "did not get every reading ($($raw[0].Count) and $($raw[1].Count) of $Samples): is the board powered and the probe on the debug header?"
}

function Get-Band([double]$mv) {
        if ($mv -lt 200) { return 'NoSource' }
        if ($mv -lt 655) { return 'Default' }
        if ($mv -lt 1235) { return '1.5 A' }
        if ($mv -lt 2200) { return '3 A' }
        return 'NoSource'
}

$mv = @{}
foreach ($c in 0, 1) {
        $sorted = @($raw[$c] | Sort-Object)
        $median = $sorted[[int][Math]::Floor($sorted.Count / 2)]
        $mv[$c] = $median * $FULL_SCALE_MV / 4095
        '{0}: {1,5:n0} mV   raw {2,4} .. {3,4}   median {4,4}   {5}' -f "CC$($c + 1)", $mv[$c], $sorted[0], $sorted[-1], $median, (Get-Band $mv[$c])
}

$line = if ($mv[0] -ge $mv[1]) { 'CC1' } else { 'CC2' }
$band = Get-Band ([Math]::Max($mv[0], $mv[1]))
$redExpected = $band -in 'Default', '1.5 A'
'decodes to: {0} on {1}' -f $band, $line
'indicators: red {0} (this reading says {1}), green {2}' -f ($red ? 'on' : 'off'), ($redExpected ? 'on' : 'off'), ($green ? 'on' : 'off')
'uptime:     {0:n1} s; {1}' -f ($timer[1] / 1e6), (($timer[1] -gt $timer[0]) ? 'running' : 'NOT RUNNING')
if ($red -ne $redExpected) { Write-Warning "the firmware's reading and this one disagree" }
