# Runs crossfire on the simulated board: the application in a desktop window, with no hardware.
#
# USAGE:  scripts/sim.ps1 [-Release]
#
#   The window shows the glass, the indicator and the BOOTSEL button, a hub with four sockets
# to plug instruments into and play them from, and the console. Console lines can also be
# piped in on standard input; the log is echoed to standard output as well as the window.
param(
        [switch]$Release
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'light-tools.ps1')

$cargo = Join-Path $env:USERPROFILE '.cargo' 'bin' 'cargo.exe'
if (-not (Test-Path $cargo)) { $cargo = 'cargo' }

$args = @('run', '-p', 'crossfire-sim')
if ($Release) { $args += '--release' }
Push-Location $root
try {
        & $cargo @args
        if ($LASTEXITCODE -ne 0) { throw "crossfire-sim exited with $LASTEXITCODE" }
} finally {
        Pop-Location
}
