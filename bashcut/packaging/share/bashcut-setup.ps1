<#
BashCutPro first-run setup (#64) for Windows: optional, every step can be skipped.

  powershell -ExecutionPolicy Bypass -File bashcut-setup.ps1 [-Yes] [-SkipLooks] [-SkipProfiles] [-SkipKit] [-SkipDoctor]

1. Looks: the curated look pack as .cube LUTs + looks.json in <data dir>\bashcut\looks (bashcut.looks.export).
2. Output profiles: compiled into export presets "BashCut <name>" (bashcut.profiles.compile).
3. Agent kit: detects claude / codex and offers to clone bashcutpro-agent-kit (needs git and access).
4. Plugins and models: installed from the app (the Plugins icon on the right-hand rail, or Window > BashCutPro > Plugins); models download on first use.
5. bashcut-doctor: checks the machine and prints fixes.
#>
param([switch]$Yes, [switch]$SkipLooks, [switch]$SkipProfiles, [switch]$SkipKit, [switch]$SkipDoctor)

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
if ($env:FILMCRAFT_DATA_DIR) { $data = $env:FILMCRAFT_DATA_DIR }
elseif (Test-Path (Join-Path $PSScriptRoot 'portable.txt')) { $data = Join-Path $PSScriptRoot 'data' }
else { $data = Join-Path $env:APPDATA 'FilmCraft' }
$cli = Find-Tool 'bashcutpro'
if (-not $cli) { $cli = Find-Tool 'filmcraft-cli' }
Write-Output "BashCutPro setup: data folder $data, command line (bashcutpro) $(if ($cli) { $cli } else { 'not found' })"
foreach ($d in 'profiles', 'templates', 'looks') { New-Item -ItemType Directory -Force -Path (Join-Path $data "bashcut\$d") | Out-Null }

if (-not $SkipLooks -and $cli -and (Ask "Write the look pack (.cube LUTs) to $data\bashcut\looks?")) {
  & $cli exec bashcut.looks.export "dir=$(Join-Path $data 'bashcut\looks')" | Out-Null
  if ($LASTEXITCODE -eq 0) { Write-Output '  looks written' } else { Write-Output '  looks: failed (skipped)' }
}
if (-not $SkipProfiles -and $cli -and (Ask 'Add the output profiles (TikTok, Reels, Shorts, YouTube...) as export presets?')) {
  $dataArgs = @()
  if (Test-Path (Join-Path $PSScriptRoot 'portable.txt')) { $dataArgs = @('--data-dir', $data) }
  & $cli @dataArgs exec bashcut.profiles.compile | Out-Null
  if ($LASTEXITCODE -eq 0) { Write-Output '  export presets "BashCut ..." added' } else { Write-Output '  profiles: failed (skipped)' }
}
if (-not $SkipKit) {
  $agents = @('claude', 'codex') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue }
  if ($agents) { Write-Output "Agent CLIs: $($agents -join ', ')" } else { Write-Output 'Agent CLIs: none found (install Claude Code or Codex to drive BashCutPro with an agent)' }
  $kit = Join-Path $env:USERPROFILE '.bashcutpro\bashcutpro-agent-kit'
  if ($agents -and -not (Test-Path $kit) -and (Get-Command git -ErrorAction SilentlyContinue) -and (Ask "Clone the agent kit to $kit?")) {
    git clone --depth 1 https://github.com/dongnguyenvie/bashcutpro-agent-kit $kit
    if ($LASTEXITCODE -eq 0) { Write-Output '  kit cloned; follow its README (register bashcutpro as an MCP server)' } else { Write-Output '  kit: clone failed (skipped)' }
  }
}
Write-Output 'Plugins: install them in the app (the Plugins icon on the right-hand rail, or Window > BashCutPro > Plugins); models download on first use.'
$doctor = Find-Tool 'bashcut-doctor'
if (-not $SkipDoctor -and $doctor) {
  Write-Output 'Checking this machine (bashcut-doctor):'
  if ($cli) { & $doctor --cli $cli } else { & $doctor }
}
