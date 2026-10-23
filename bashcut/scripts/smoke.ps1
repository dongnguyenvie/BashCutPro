<#
BashCutPro smoke check (#54) for Windows builds. macOS / Linux: smoke.sh.

Launches the built app with --control, waits for its window, takes ui.screenshot, checks the window title says
BashCutPro, runs `filmcraft-cli commands razor` and an MCP command_list, quits the app; then scans the x86-64 release
binaries for AVX / AVX2 / AVX-512 instructions outside the allowed (runtime-dispatched) functions.
Exits non-zero on any failure and never reports success without having launched the app.

  powershell -ExecutionPolicy Bypass -File bashcut\scripts\smoke.ps1 -App target\release\filmcraft.exe
  powershell -ExecutionPolicy Bypass -File bashcut\scripts\smoke.ps1 -IsaOnly target\release\filmcraft.exe,target\release\filmcraft-cli.exe

Options: -Cli <path> (default: filmcraft-cli.exe, bashcutpro.exe or bcpro.exe next to the app, else on PATH), -Port N (a free port from 19000), -Timeout S (60),
         -Out DIR (target\smoke), -ExpectTitle TEXT (BashCutPro), -NoIsa, -IsaAllow FILE (bashcut\scripts\isa-allow.txt)
Needs for the scan: llvm-objdump (PATH, or `rustup component add llvm-tools`) or dumpbin (Visual Studio).
Windows PowerShell 5.1 or PowerShell 7.
#>
param(
    [string]$App = "",
    [string]$Cli = "",
    [int]$Port = 0,
    [int]$Timeout = 60,
    [string]$Out = "target\smoke",
    [string]$ExpectTitle = "BashCutPro",
    [switch]$NoIsa,
    [string]$IsaAllow = "",
    [string[]]$IsaOnly = @()
)

Set-StrictMode -Version Latest
# Stop on any error, but every step below runs inside try/catch, so a failed step is a FAIL line and the next step
# still runs (like smoke.sh); the summary and the exit code count them.
$ErrorActionPreference = "Stop"
$script:passes = 0
$script:failures = 0
function Pass([string]$m) { $script:passes++; Write-Output "PASS  $m" }
function Fail([string]$m) { $script:failures++; Write-Output "FAIL  $m" }
function Note([string]$m) { Write-Output "      $m" }
if (-not $IsaAllow) { $IsaAllow = Join-Path $PSScriptRoot "isa-allow.txt" }

# ------------------------------------------------------------------------------------------------ ISA scan
function Find-Disassembler {
    $ErrorActionPreference = "Continue"  # rustc may write to stderr; that is not a failure here
    $c = Get-Command llvm-objdump -ErrorAction SilentlyContinue
    if ($c) { return @{ kind = "llvm"; path = $c.Source } }
    $sysroot = $null
    try { $sysroot = (& rustc --print sysroot 2>$null) } catch { }
    if ($sysroot) {
        $p = Get-ChildItem -Path (Join-Path $sysroot "lib\rustlib") -Filter llvm-objdump.exe -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($p) { return @{ kind = "llvm"; path = $p.FullName } }
    }
    $d = Get-Command dumpbin -ErrorAction SilentlyContinue
    if ($d) { return @{ kind = "dumpbin"; path = $d.Source } }
    return $null
}

function Test-X64([string]$bin) {
    # PE header: e_lfanew at 0x3C, then "PE\0\0" and the machine (0x8664 = x86-64)
    try {
        $fs = [IO.File]::OpenRead($bin)
        $br = New-Object IO.BinaryReader($fs)
        $fs.Seek(0x3C, 'Begin') | Out-Null
        $pe = $br.ReadInt32()
        $fs.Seek($pe + 4, 'Begin') | Out-Null
        $machine = $br.ReadUInt16()
        $fs.Close()
        return ($machine -eq 0x8664)
    } catch { return $false }
}

function Invoke-IsaScan([string]$bin) {
    if (-not (Test-Path $bin -PathType Leaf)) { Fail "isa: $bin not found"; return }
    if (-not (Test-X64 $bin)) { Note "isa: $bin is not an x86-64 executable; skipped"; return }
    # Windows PowerShell 5.1 turns stderr of a native tool into errors (`2>$null` included): judge the tools by output
    $ErrorActionPreference = "Continue"
    $dis = Find-Disassembler
    if (-not $dis) { Fail "isa: no llvm-objdump or dumpbin to disassemble $bin (rustup component add llvm-tools)"; return }
    $patterns = @()
    if (Test-Path $IsaAllow) {
        $patterns = Get-Content $IsaAllow | Where-Object { $_ -and -not $_.StartsWith("#") }
    }
    if ($dis.kind -eq "llvm") {
        $lines = & $dis.path -d --no-show-raw-insn $bin 2>$null
        $header = '^[0-9a-f]+ <(.+)>:$'
    } else {
        $lines = & $dis.path /nologo /disasm:nobytes $bin 2>$null
        $header = '^(\S+):$'
    }
    $wide = 'ymm\d|zmm\d|%k[1-7]\b|\{k[1-7]\}'
    $fn = "?"
    $hits = New-Object System.Collections.Generic.List[object]
    foreach ($l in $lines) {
        if ($l -match $header) { $fn = $Matches[1]; continue }
        if ($l -notmatch $wide) { continue }
        $kind = "AVX/AVX2"
        if ($l -match 'zmm\d|%k[1-7]\b|\{k[1-7]\}') { $kind = "AVX-512" }
        $addr = if ($l -match '^\s*([0-9a-f]+):') { $Matches[1] } else { "" }
        $hits.Add(@{ kind = $kind; fn = $fn; addr = $addr })
    }
    # A PE keeps its function names in the PDB, which llvm-objdump does not read: it only prints
    # section headers (`<.text>`). Name each hit through llvm-symbolizer, which reads the PDB. With line tables the
    # PDB knows inlined frames, and llvm-symbolizer still reports them under --no-inlines (an intrinsic such as
    # _mm256_add_ps, never its `*_avx2` caller): take the outermost frame of --inlines instead (JSON, one line per
    # address). Without debug info the PDB has only public symbols, so a hit is named after the nearest one before it.
    $sections = @($hits | Where-Object { $_.fn -match '^\.' -and $_.addr })
    if ($dis.kind -eq "llvm" -and $sections.Count -gt 0) {
        $sym = Get-Command llvm-symbolizer -ErrorAction SilentlyContinue
        $symPath = if ($sym) { $sym.Source } else { Join-Path (Split-Path $dis.path) "llvm-symbolizer.exe" }
        if (Test-Path $symPath) {
            # PowerShell may put a BOM before the first line it pipes to a native program, which spoils that address:
            # lead with a dummy one and drop its answer
            $names = @(@("0x0") + @($sections | ForEach-Object { "0x$($_.addr)" }) | & $symPath "--obj=$bin" --functions=linkage --inlines --output-style=JSON 2>$null)
            $names = @($names | Select-Object -Skip 1)
            $i = 0
            $lineInfo = $false
            foreach ($n in @($names)) {
                if (-not $n -or $i -ge $sections.Count) { continue }
                try { $frames = @(($n | ConvertFrom-Json).Symbol) } catch { $frames = @() }
                $outer = if ($frames.Count -gt 0) { $frames[$frames.Count - 1] } else { $null }
                if ($outer -and $outer.FunctionName -and $outer.FunctionName -ne "??") { $sections[$i].fn = $outer.FunctionName }
                if ($outer -and $outer.FileName) { $lineInfo = $true }
                $i++
            }
            if (-not $lineInfo) {
                Note "isa: $bin has no line tables in its PDB: hits are named after the nearest public symbol, which may be a neighbour (exact names: build with `$env:CARGO_PROFILE_RELEASE_DEBUG = 'line-tables-only')"
            }
        } else {
            Note "isa: no llvm-symbolizer next to $($dis.path): hits are named by section only"
        }
    }
    $seen = @{}
    foreach ($h in $hits) {
        $ok = $false
        foreach ($p in $patterns) { if ($h.fn -match $p) { $ok = $true; break } }
        if (-not $ok) { $seen["$($h.kind)`t$($h.fn)"] = $true }
    }
    if ($seen.Count -eq 0) {
        Pass "isa: $bin has no AVX/AVX2/AVX-512 outside allowed functions"
    } else {
        Fail "isa: $bin uses wide vector instructions in $($seen.Count) function(s):"
        $seen.Keys | Sort-Object | Select-Object -First 20 | ForEach-Object { Note $_ }
    }
}

function Write-Summary([string]$what) {
    Write-Output ("smoke{0}: {1} passed, {2} failed (Windows {3}, {4})" -f $what, $script:passes, $script:failures, $env:PROCESSOR_ARCHITECTURE, (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mmZ"))
}

# `-File smoke.ps1 -IsaOnly a.exe,b.exe` hands over one string: split it
$IsaOnly = @($IsaOnly | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
if ($IsaOnly.Count -gt 0) {
    foreach ($b in $IsaOnly) { try { Invoke-IsaScan $b } catch { Fail "isa: $($b): $_" } }
    Write-Summary " (isa)"
    if ($script:failures -eq 0) { exit 0 } else { exit 1 }
}

# ------------------------------------------------------------------------------------------------ launch
if (-not $App) { [Console]::Error.WriteLine("smoke: -App <path to the built app> is required"); exit 2 }
if (-not (Test-Path $App -PathType Leaf)) { Fail "launch: $App is not a file"; Write-Summary ""; exit 1 }
$App = (Resolve-Path $App).Path
if (-not $Cli) {
    # filmcraft-cli, or BashCutPro's bashcutpro / bcpro (-p bashcutpro-cli), next to the app, else on PATH
    $names = @("filmcraft-cli", "bashcutpro", "bcpro")
    foreach ($n in $names) {
        $cand = Join-Path (Split-Path $App) "$n.exe"
        if (Test-Path $cand -PathType Leaf) { $Cli = $cand; break }
    }
    foreach ($n in $names) {
        if ($Cli) { break }
        $c = Get-Command $n -ErrorAction SilentlyContinue
        if ($c) { $Cli = $c.Source }
    }
}
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path

function Test-PortOpen([int]$p) {
    $c = New-Object Net.Sockets.TcpClient
    try { $c.Connect("127.0.0.1", $p); return $true } catch { return $false } finally { $c.Close() }
}
if ($Port -eq 0) {
    $Port = 19000
    while (Test-PortOpen $Port) { $Port++ }
}

# One request on a fresh connection; returns the reply line ("" on failure).
function Send-Request([string]$json, [int]$seconds = 30) {
    $c = New-Object Net.Sockets.TcpClient
    try {
        $c.Connect("127.0.0.1", $Port)
        $c.ReceiveTimeout = $seconds * 1000
        $s = $c.GetStream()
        $w = New-Object IO.StreamWriter($s)
        $w.NewLine = "`n"
        $w.WriteLine($json)
        $w.Flush()
        $r = New-Object IO.StreamReader($s)
        return [string]$r.ReadLine()
    } catch { return "" } finally { $c.Close() }
}

Write-Output "smoke: $App on port $Port (cli: $Cli, out: $Out)"
$proc = $null
$ready = $false
try {
    $proc = Start-Process -FilePath $App -ArgumentList @("--control", "$Port", "--control-input-only") -PassThru `
        -RedirectStandardOutput (Join-Path $Out "app.log") -RedirectStandardError (Join-Path $Out "app.err.log")
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ((Get-Date) -lt $deadline) {
        if ($proc.HasExited) { break }
        $r = Send-Request '{"id":1,"method":"ui.inspect"}' 5
        if ($r -match '"ok":\s*true') { $ready = $true; break }
        Start-Sleep -Milliseconds 500
    }
} catch { Note "launch: $_" }
if (-not $ready) {
    Fail "launch: no answer on the control port within $Timeout s (log: $Out\app.log)"
    Get-Content (Join-Path $Out "app.err.log") -Tail 20 -ErrorAction SilentlyContinue | ForEach-Object { Note $_ }
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Write-Summary ""
    exit 1
}
Pass "launch: the app answers ui.inspect (pid $($proc.Id))"

# ------------------------------------------------------------------------------------------------ window
$shot = Join-Path $Out "smoke.png"
try {
    Remove-Item $shot -ErrorAction SilentlyContinue
    $shotJson = ConvertTo-Json -Compress @{ id = 2; method = "ui.screenshot"; params = @{ path = $shot } }
    $r = Send-Request $shotJson 30
    $isPng = $false
    if (Test-Path $shot) {
        $b = [IO.File]::ReadAllBytes($shot)
        $isPng = ($b.Length -gt 8 -and $b[0] -eq 0x89 -and $b[1] -eq 0x50 -and $b[2] -eq 0x4E -and $b[3] -eq 0x47)
    }
    if ($isPng) { Pass "screenshot: $shot" } else { Fail "screenshot: no PNG written ($r)" }
} catch { Fail "screenshot: $_" }

try {
    $title = ""
    for ($i = 0; $i -lt 20 -and -not $title; $i++) {
        $proc.Refresh()
        $title = $proc.MainWindowTitle
        if (-not $title) { Start-Sleep -Milliseconds 250 }
    }
    if ($title) {
        if ($title -like "*$ExpectTitle*") { Pass "title: `"$title`"" } else { Fail "title: `"$title`" does not say $ExpectTitle" }
    } else {
        $els = Send-Request '{"id":3,"method":"ui.elements"}' 30
        if ($els -like "*$ExpectTitle*") { Pass "title: no window title; the UI shows $ExpectTitle" } else { Fail "title: no window title and the UI does not show $ExpectTitle" }
    }
} catch { Fail "title: $_" }

# ------------------------------------------------------------------------------------------------ CLI and MCP
# Run a program with a time limit; stdout goes to $outFile. Returns the exit code (124 = timed out).
function Invoke-Limited([string]$exe, [string[]]$argv, [string]$outFile, [string]$inFile = "", [int]$seconds = 60) {
    # the CLI checks test the engine itself, not the app launched above (CC3); the child inherits it
    $bridge = $env:BASHCUT_BRIDGE
    $env:BASHCUT_BRIDGE = "off"
    try { return Invoke-LimitedOnce $exe $argv $outFile $inFile $seconds } finally { $env:BASHCUT_BRIDGE = $bridge }
}

function Invoke-LimitedOnce([string]$exe, [string[]]$argv, [string]$outFile, [string]$inFile, [int]$seconds) {
    $p = @{ FilePath = $exe; ArgumentList = $argv; PassThru = $true; NoNewWindow = $true; RedirectStandardOutput = $outFile; RedirectStandardError = "$outFile.err" }
    if ($inFile) { $p.RedirectStandardInput = $inFile }
    $q = Start-Process @p
    # Windows PowerShell 5.1 only keeps ExitCode for a process whose handle was read while it ran
    $null = $q.Handle
    if (-not $q.WaitForExit($seconds * 1000)) { Stop-Process -Id $q.Id -Force -ErrorAction SilentlyContinue; return 124 }
    return $q.ExitCode
}

if (-not $Cli -or -not (Test-Path $Cli)) {
    Fail "cli: filmcraft-cli / bashcutpro not found (pass -Cli)"
} else {
    try {
        $razor = Join-Path $Out "cli-razor.txt"
        $code = Invoke-Limited $Cli @("commands", "razor") $razor
        if ($code -eq 0 -and (Select-String -Path $razor -Pattern "razor" -Quiet)) {
            Pass "cli: commands razor ($(@(Get-Content $razor).Count) line(s))"
        } else {
            Fail "cli: commands razor failed (exit $code, $razor)"
        }
    } catch { Fail "cli: commands razor: $_" }
    try {
        $mcpIn = Join-Path $Out "mcp-in.jsonl"
        $mcpOut = Join-Path $Out "mcp-out.jsonl"
        $lines = @(
            '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}',
            '{"jsonrpc":"2.0","method":"notifications/initialized"}',
            '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"command_list","arguments":{}}}'
        )
        [IO.File]::WriteAllText($mcpIn, (($lines -join "`n") + "`n"))
        Invoke-Limited $Cli @("--demo", "mcp") $mcpOut $mcpIn 60 | Out-Null
        $reply = Get-Content $mcpOut -ErrorAction SilentlyContinue | Where-Object { $_ -match '"id":\s*2' } | Select-Object -First 1
        if ($reply -and $reply -match 'file\.newProject' -and $reply -notmatch '"isError":\s*true') {
            Pass "mcp: command_list answers"
        } else {
            Fail "mcp: command_list gave no command list ($mcpOut)"
        }
    } catch { Fail "mcp: command_list: $_" }
}

# ------------------------------------------------------------------------------------------------ quit
Send-Request '{"id":9,"method":"app.quit"}' 10 | Out-Null
if ($proc.WaitForExit(15000)) {
    Pass "quit: the app exited"
} else {
    Fail "quit: the app is still running 15 s after app.quit"
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
}

# ------------------------------------------------------------------------------------------------ ISA
if (-not $NoIsa) {
    foreach ($b in @($App, $Cli)) {
        if ($b -and (Test-Path $b)) { try { Invoke-IsaScan $b } catch { Fail "isa: $($b): $_" } }
    }
}

Write-Summary ""
if ($script:failures -eq 0) { exit 0 } else { exit 1 }
