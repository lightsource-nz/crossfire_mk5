# Measures a crossfire board from its ports: latency one way and round trip, and throughput.
#
# THE SETUP: the bench's two boards on two of crossfire's ports and nothing else on it, with GP2
# wired to GP2 between them and a ground wire beside it.
#   - the TIMER, an RP2350-Zero (target crossfire_bench_timer), with a debug probe on its SWD pads;
#   - the ECHO, an RP2040-Zero (target crossfire_bench_echo), written once in BOOTSEL from the
#     computer and then moved over. GP3 held to ground at its power-up gives it a four-byte OUT
#     endpoint, an instrument that takes one event at a time.
# crates/crossfire-bench says what is measured and how.
#
# WHAT THIS DOES: restarts the timer (or writes it first, with -Flash), waits for its run to finish,
# and reads its figures over SWD. NOTHING IS HALTED while it reads: OpenOCD alone, with no gdb, reads
# the block of words the timer keeps up to date, so the run is not disturbed by being watched. The
# block is found by its symbol in the timer's ELF, and its layout is checked by its magic and version.
#
# USAGE:  scripts/bench.ps1 [-Flash] [-NoRestart] [-TimeoutSeconds 90]
#         -NoRestart reads the run that is there, finished or not, rather than starting another.
param(
        [switch]$Flash,
        [switch]$NoRestart,
        [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'light-tools.ps1')
Import-Module (Join-Path $LightScripts 'lib/LightProject.psm1') -Force
Import-Module (Join-Path $LightScripts 'lib/LightPlatform.psm1') -Force

$TARGET = 'crossfire_bench_timer'
$PRESET = 'conf-crossfire-bench-timer-debug'
#   the layout of crossfire_bench::timer::Results, VERSION 1: keep in step with it
$MAGIC = 0x4E424658
$VERSION = 1
#   (PowerShell names ignore case: this must not be spelled like any variable below)
$BLOCK_WORDS = 116
$PHASES = @('connecting', 'latency', 'rates', 'done')

$config = Get-LightProjectConfig -ProjectRoot $root
$tree = Join-Path $root $config.Trees[$PRESET]
$elf = Join-Path $tree "module/$TARGET/$TARGET.elf"
$debug = $config.Debug[$PRESET]

if ($Flash) {
        & (Join-Path $PSScriptRoot 'flash.ps1') -Target $TARGET
        if ($LASTEXITCODE) { throw "writing the timer failed" }
}
if (-not (Test-Path $elf)) { throw "no timer image at '$elf': build it with scripts/build.ps1 -Target $TARGET, or run this with -Flash" }

$cache = Join-Path $tree 'CMakeCache.txt'
$nm = (Select-String -Path $cache -Pattern '^CMAKE_NM:FILEPATH=(.*)$').Matches[0].Groups[1].Value
$symbol = & $nm $elf | Select-String ' CROSSFIRE_BENCH_RESULTS$' | Select-Object -First 1
if (-not $symbol) { throw "the timer image has no CROSSFIRE_BENCH_RESULTS: is '$elf' the bench timer?" }
$address = [Convert]::ToInt64(($symbol.Line -split ' ')[0], 16)

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
$base = @()
if ($searchDir) { $base += @('-s', $searchDir) }
$base += @('-f', (Join-Path $root $debug.Config), '-c', 'init')

function Invoke-Ocd([string[]]$commands) {
        $a = $base + ($commands | ForEach-Object { @('-c', $_) }) + @('-c', 'shutdown')
        & $openocd @a 2>&1 | ForEach-Object { "$_" }
}

function Read-Results {
        $out = Invoke-Ocd @(('mdw 0x{0:x8} {1}' -f $address, $BLOCK_WORDS))
        $words = @()
        foreach ($line in $out) {
                if ($line -notmatch '^0x[0-9a-f]{8}:((\s+[0-9a-f]{8})+)') { continue }
                $words += $Matches[1].Trim() -split '\s+' | ForEach-Object { [Convert]::ToInt64($_, 16) }
        }
        if ($words.Count -ne $BLOCK_WORDS) {
                $out | Select-Object -Last 12 | ForEach-Object { Write-Host $_ }
                throw "read $($words.Count) of $BLOCK_WORDS words: is the probe on the timer's SWD pads, and the timer powered?"
        }
        if ($words[0] -ne $MAGIC) { throw ('the block at 0x{0:x8} does not start with the bench magic (read 0x{1:x8}): is the timer running the image that was built?' -f $address, $words[0]) }
        if ($words[1] -ne $VERSION) { throw "the timer's figures are version $($words[1]); this script reads version $VERSION" }
        return , $words
}

function Get-Summary($w, [int]$at) {
        [pscustomobject]@{ Count = $w[$at]; Min = $w[$at + 1]; P50 = $w[$at + 2]; P90 = $w[$at + 3]; P99 = $w[$at + 4]; Max = $w[$at + 5] }
}

if (-not $NoRestart) {
        Write-Host "restarting the timer for a new run"
        Invoke-Ocd @('reset run') | Out-Null
}

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$shown = -1
do {
        Start-Sleep -Milliseconds 1000
        $w = Read-Results
        $phase = [int]$w[2]
        $progress = switch ($phase) {
                0 { 'waiting for the first echo' }
                1 { "latency: $($w[4]) probes" }
                2 { "rates: $($w[19]) done" }
                default { 'done' }
        }
        if ($phase -ne $shown -or $phase -lt 3) { Write-Host "  $($PHASES[[Math]::Min($phase, 3)]): $progress" }
        $shown = $phase
        if ($NoRestart) { break }
} while ($phase -lt 3 -and (Get-Date) -lt $deadline)

if ($phase -eq 0) {
        throw "no echo came back: is the echo on another of crossfire's ports, running crossfire_bench_echo, and nothing else plugged in?"
}

$oneWay = Get-Summary $w 7
$roundTrip = Get-Summary $w 13
''
'latency, {0} messages one at a time ({1} without an edge, {2} without an echo):' -f $w[4], $w[6], $w[5]
'  {0,-11} {1,8} {2,8} {3,8} {4,8} {5,8}' -f '', 'min', 'p50', 'p90', 'p99', 'max'
foreach ($row in @(@('one way', $oneWay), @('round trip', $roundTrip))) {
        $s = $row[1]
        '  {0,-11} {1,8:n0} {2,8:n0} {3,8:n0} {4,8:n0} {5,8:n0}  us' -f $row[0], $s.Min, $s.P50, $s.P90, $s.P99, $s.Max
}
''
'throughput, each rate for two seconds (round trips in us):'
'  {0,8} {1,8} {2,8} {3,8} {4,8} {5,8} {6,8} {7,8} {8,8}' -f 'rate/s', 'offered', 'sent', 'echoed', 'skipped', 'behind', 'p50', 'p99', 'max'
for ($i = 0; $i -lt $w[19]; $i++) {
        $at = 20 + $i * 12
        $s = Get-Summary $w ($at + 6)
        '  {0,8:n0} {1,8:n0} {2,8:n0} {3,8:n0} {4,8:n0} {5,8:n0} {6,8:n0} {7,8:n0} {8,8:n0}' -f $w[$at], $w[$at + 1], $w[$at + 2], $w[$at + 3], $w[$at + 4], $w[$at + 5], $s.P50, $s.P99, $s.Max
}
''
'resolution: the timer loop''s longest pass was {0} us; percentiles are to 10 us' -f $w[3]
if ($phase -lt 3) { Write-Warning "the run had not finished: these are the figures so far" }
