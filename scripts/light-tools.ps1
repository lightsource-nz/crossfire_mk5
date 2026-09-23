# Sets $LightScripts for the wrappers in this directory.
#
# This project is NOT the framework: the shared light-*.ps1 layer lives in the light framework
# checkout, found through LIGHT_PATH (the environment, or the sibling directory by default). The
# wrappers here supply only this project's defaults and call that layer for the logic.
$ErrorActionPreference = 'Stop'

$LightPath = if ($env:LIGHT_PATH) { $env:LIGHT_PATH } else { Join-Path $PSScriptRoot '..' '..' 'light_mk5' }
$LightPath = (Resolve-Path $LightPath).Path
if (-not (Test-Path (Join-Path $LightPath 'light_init.cmake'))) {
        throw "could not find the light framework at '$LightPath' -- it is normally the light_mk5 checkout beside this project; set LIGHT_PATH for another layout."
}
$env:LIGHT_PATH = $LightPath
$LightScripts = Join-Path $LightPath 'scripts'
