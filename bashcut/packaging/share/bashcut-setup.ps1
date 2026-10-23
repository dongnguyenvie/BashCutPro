<#
BashCutPro first-run setup (#64) for Windows: optional, every step can be skipped.

  powershell -ExecutionPolicy Bypass -File bashcut-setup.ps1 [-Yes] [-SkipLooks] [-SkipProfiles] [-SkipKit] [-SkipDoctor]

1. Looks: the curated look pack as .cube LUTs + looks.json in <data dir>\bashcut\looks (bashcut.looks.export).
2. Output profiles: compiled into export presets "BashCut <name>" (bashcut.profiles.compile).
3. Agent kit: installs it as a Claude Code plugin when claude is found; clones it for Codex (needs access to the kit repository).
4. Plugins and models: installed from the app (the Plugins icon on the right-hand rail, or Window > BashCutPro > Plugins); models download on first use.
5. bashcut-doctor: checks the machine and prints fixes.
#>
param([switch]$Yes, [switch]$SkipLooks, [switch]$SkipProfiles, [switch]$SkipKit, [switch]$SkipDoctor)
Set-StrictMode -Version Latest
# Stop on any error, but each optional step runs inside try/catch: a failed one prints "failed (skipped)" and the
# next one still runs (like bashcut-setup.sh).
$ErrorActionPreference = 'Stop'

function Ask([string]$q) {
  if ($Yes) { return $true }
  $r = Read-Host "$q [y/N]"
  return ($r -match '^(y|yes)$')
}
function Find-Tool([string]$name) {
  $p = Join-Path $PSScriptRoot "$name.exe"
  if (Test-Path $p) { return $p }
  $c = Get-Command $name -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  return $null
}

# Portable installs keep their data in .\data next to the exe (portable.txt), like the app.
# BashCutPro's own data folder, the app's rules (bashcut_dirs::data_dir, SB1); not FilmCraft's
if ($env:BASHCUTPRO_DATA_DIR) { $data = $env:BASHCUTPRO_DATA_DIR }
elseif ($env:FILMCRAFT_DATA_DIR) { $data = $env:FILMCRAFT_DATA_DIR }
elseif (Test-Path (Join-Path $PSScriptRoot 'portable.txt')) { $data = Join-Path $PSScriptRoot 'data' }
else { $data = Join-Path $env:APPDATA 'BashCutPro' }
# the engine: bashcutpro-cli, its alias bcpro, upstream's filmcraft-cli (NM2; bashcutpro.exe is the app)
$cli = Find-Tool 'bashcutpro-cli'
if (-not $cli) { $cli = Find-Tool 'bcpro' }
if (-not $cli) { $cli = Find-Tool 'filmcraft-cli' }
Write-Output "BashCutPro setup: data folder $data, command line (bashcutpro-cli) $(if ($cli) { $cli } else { 'not found' })"
foreach ($d in 'profiles', 'templates', 'looks') { New-Item -ItemType Directory -Force -Path (Join-Path $data "bashcut\$d") | Out-Null }

# looks and profiles are written by the headless engine, not the open app (CC3); restored below
$bridge = $env:BASHCUT_BRIDGE
$env:BASHCUT_BRIDGE = 'off'
if (-not $SkipLooks -and $cli -and (Ask "Write the look pack (.cube LUTs) to $data\bashcut\looks?")) {
  try {
    & $cli exec bashcut.looks.export "dir=$(Join-Path $data 'bashcut\looks')" | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Output '  looks written' } else { Write-Output '  looks: failed (skipped)' }
  } catch { Write-Output "  looks: failed (skipped): $_" }
}
if (-not $SkipProfiles -and $cli -and (Ask 'Add the output profiles (TikTok, Reels, Shorts, YouTube...) as export presets?')) {
  try {
    $dataArgs = @()
    if (Test-Path (Join-Path $PSScriptRoot 'portable.txt')) { $dataArgs = @('--data-dir', $data) }
    & $cli @dataArgs exec bashcut.profiles.compile | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Output '  export presets "BashCut ..." added' } else { Write-Output '  profiles: failed (skipped)' }
  } catch { Write-Output "  profiles: failed (skipped): $_" }
}
$env:BASHCUT_BRIDGE = $bridge
if (-not $SkipKit) {
  $agents = @(@('claude', 'codex') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue })
  if ($agents.Count -gt 0) { Write-Output "Agent CLIs: $($agents -join ', ')" } else { Write-Output 'Agent CLIs: none found (install Claude Code or Codex to drive BashCutPro with an agent)' }
  # Claude Code: install the kit as its plugin (the kit README's way); Codex: clone it (see the kit's setup skill)
  if ((Get-Command claude -ErrorAction SilentlyContinue) -and (Ask 'Install the agent kit as a Claude Code plugin (bcp)?')) {
    try {
      # Windows PowerShell 5.1 turns redirected stderr of a native command into errors: judge it by the exit code
      $ErrorActionPreference = 'Continue'
      claude plugin marketplace add dongnguyenvie/bashcutpro-agent-kit *> $null
      if ($LASTEXITCODE -eq 0) { claude plugin install bcp@bashcutpro-agent-kit *> $null }
      $ErrorActionPreference = 'Stop'
      if ($LASTEXITCODE -eq 0) { Write-Output '  kit installed in Claude Code; in Claude Code run /bcp:setup once' } else { Write-Output '  kit: Claude Code plugin install failed (skipped); see the kit README' }
    } catch { Write-Output "  kit: Claude Code plugin install failed (skipped): $_" }
    $ErrorActionPreference = 'Stop'
  }
  $kit = Join-Path $env:USERPROFILE '.bashcutpro\bashcutpro-agent-kit'
  if ((Get-Command codex -ErrorAction SilentlyContinue) -and -not (Test-Path $kit) -and (Get-Command git -ErrorAction SilentlyContinue) -and (Ask "Clone the agent kit for Codex to $kit?")) {
    try {
      $ErrorActionPreference = 'Continue'  # git prints progress on stderr
      git clone --depth 1 https://github.com/dongnguyenvie/bashcutpro-agent-kit $kit
      $ErrorActionPreference = 'Stop'
      if ($LASTEXITCODE -eq 0) { Write-Output '  kit cloned; for Codex on Windows follow its setup skill (skills/setup/SKILL.md)' } else { Write-Output '  kit: clone failed (skipped)' }
    } catch { Write-Output "  kit: clone failed (skipped): $_" }
    $ErrorActionPreference = 'Stop'
  }
}
Write-Output 'Plugins: install them in the app (the Plugins icon on the right-hand rail, or Window > BashCutPro > Plugins); models download on first use.'
$doctor = Find-Tool 'bashcut-doctor'
if (-not $SkipDoctor -and $doctor) {
  Write-Output 'Checking this machine (bashcut-doctor):'
  try {
    if ($cli) { & $doctor --cli $cli } else { & $doctor }
  } catch { Write-Output "  bashcut-doctor: failed (skipped): $_" }
}
