<#
BashCutPro: package for Windows with the Pro pieces (#64, bashcut/packaging/README.md).

Called by the hook at the top of packaging/windows/package.ps1 with the same parameters: writes a copy of packaging/
with our additions (generate.py) and runs the copy's package.ps1. $env:BASHCUT_PACKAGING_PLAIN = '1' skips this and
packages upstream's files as they are.
#>
param(
  [string] $Arch = 'x64',
  [switch] $SkipBuild
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$Out = Join-Path $TargetDir 'bashcut-packaging'
# the first Python that runs: on Windows `python3` is often the Microsoft Store alias, which only prints a hint
$Python = $null
foreach ($Candidate in @(@{ Exe = 'py'; Args = @('-3') }, @{ Exe = 'python3'; Args = @() }, @{ Exe = 'python'; Args = @() })) {
  if (-not (Get-Command $Candidate.Exe -ErrorAction SilentlyContinue)) { continue }
  $CandidateArgs = $Candidate.Args
  try { & $Candidate.Exe @CandidateArgs --version *> $null } catch { continue }
  if ($LASTEXITCODE -eq 0) { $Python = $Candidate; break }
}
if (-not $Python) { throw 'Python 3 was not found: install it from python.org (the Microsoft Store "python3" alias is not enough) and run again' }
$PythonArgs = $Python.Args
& $Python.Exe @PythonArgs (Join-Path $PSScriptRoot 'generate.py') --root $Root --out $Out
if ($LASTEXITCODE -ne 0) { throw "bashcut/packaging/generate.py failed ($LASTEXITCODE)" }
$env:BASHCUT_BRANDED = '1'
$env:BASHCUT_ROOT = $Root
$env:BASHCUT_PACKAGING = $Out
& (Join-Path $Out 'windows\package.ps1') @PSBoundParameters
$global:LASTEXITCODE = 0
