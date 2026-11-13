# Gives a running board new firmware over its console, into the slot it is not running: the
# wired route of the A/B update, for the production board, which has no radio. The board must
# already run firmware that answers `receive`; flash.ps1 puts that on once.
#
# USAGE:  scripts/update.ps1 [-Target <name>] [-NoBuild] [-Image <file>] [-Port COM21]
param(
        [string]$Target,
        [string]$Preset,
        [switch]$NoBuild,
        [string]$Image,
        [string]$Port,
        [string]$Board
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'light-tools.ps1')

if (-not $Target) { $Target = (& (Join-Path $PSScriptRoot 'project.config.ps1')).DefaultTarget }

& (Join-Path $LightScripts 'light-update.ps1') -Target $Target -Preset $Preset -NoBuild:$NoBuild -Image $Image -Port $Port -Board $Board -ProjectRoot $root
