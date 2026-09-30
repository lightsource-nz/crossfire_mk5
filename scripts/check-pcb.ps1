# Checks the schematic for the faults a rules check cannot see -- a pin wired to a stub that goes
# nowhere, a part bridged by a wire across its own pins, a wire end off the grid. Each of these is
# legal, so the rules check passes it, and each of them changes the netlist.
#
# Unlike the other wrappers here this one is self-contained: it is about this project's board, not
# about the framework, so there is no shared layer to call. It uses the Python that ships with
# KiCad, which is the one guaranteed to be present wherever kicad-cli is.
#
# Exits non-zero when it finds something, so it can gate a commit or a build.
#
# USAGE:  scripts/check-pcb.ps1
$ErrorActionPreference = 'Stop'

$python = $null
foreach ($base in @("$env:ProgramFiles\KiCad", "${env:ProgramFiles(x86)}\KiCad")) {
	if (-not (Test-Path $base)) { continue }
	foreach ($ver in (Get-ChildItem $base -Directory | Sort-Object Name -Descending)) {
		$candidate = Join-Path $ver.FullName 'bin\python.exe'
		if (Test-Path $candidate) { $python = $candidate; break }
	}
	if ($python) { break }
}
if (-not $python) {
	$fallback = Get-Command python -ErrorAction SilentlyContinue
	if ($fallback) { $python = $fallback.Source }
}
if (-not $python) {
	throw "could not find a Python to run the check with -- KiCad ships one at <KiCad>\bin\python.exe"
}

& $python (Join-Path $PSScriptRoot 'check-pcb.py')
exit $LASTEXITCODE
