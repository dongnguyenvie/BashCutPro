# Smoke launch and instruction-set checks (#54)

Catch broken builds before users do. Run by hand (workflows stay manual) on mac 01 and geekomT before each release,
and log the summary line in [status.md](status.md) § Smoke checks.

| Script | OS |
|---|---|
| `bashcut/scripts/smoke.sh --app <filmcraft or BashCutPro.app> [--cli <filmcraft-cli>]` | macOS, Linux (bash) |
| `powershell -ExecutionPolicy Bypass -File bashcut\scripts\smoke.ps1 -App <filmcraft.exe> [-Cli <filmcraft-cli.exe>]` | Windows (PowerShell 5.1 / 7) |

Steps, each printed as `PASS` / `FAIL`; the exit status is 1 when any step failed, and nothing passes unless the app
was launched and answered:

1. Launch the app with `--control <free port> --control-input-only`; wait (`--timeout`, 60 s) until `ui.inspect`
   answers on the control port.
2. `ui.screenshot` to `<out>/smoke.png` and check it is a PNG.
3. The window title must contain `BashCutPro` (`--expect-title`): macOS through System Events (Terminal needs
   Accessibility permission), Linux through `xdotool` / `wmctrl`, Windows through the process's main window title. When
   no title can be read, the UI's elements (`ui.elements`) must show the name.
4. `filmcraft-cli commands razor` lists the razor commands; `filmcraft-cli --demo mcp` answers `tools/call command_list`.
   Without `--cli` / `-Cli` the CLI is `bashcutpro-cli`, `bcpro` or `filmcraft-cli` next to the app (in that order), else on
   PATH; `cargo build --release -p filmcraft -p bashcutpro-cli` gives `bashcutpro-cli`. Both run with `BASHCUT_BRIDGE=off`, so
   they test the headless engine even when another BashCutPro is open.
5. `app.quit` (control method: no save prompt); the process must exit within 15 s.
6. Instruction-set scan of the app and CLI when they are x86-64: no `ymm` (AVX / AVX2) or `zmm` / opmask (AVX-512)
   registers outside the functions matched by `bashcut/scripts/isa-allow.txt` (runtime-dispatched code only). Needs
   `llvm-objdump` (`rustup component add llvm-tools`) or `objdump`; on Windows `dumpbin` also works. On Windows the function
   names live in the PDB, which `llvm-objdump` does not read, so each hit is named through `llvm-symbolizer` (next to
   `llvm-objdump`, or on PATH; rustup's llvm-tools has none, an LLVM or Swift toolchain does) as the outermost frame of
   its inline chain. A plain release PDB holds only public symbols, so hits get a neighbour's name (memchr's
   `find_avx2` shows up as `detect`) and the scan fails; build the release to scan with
   `$env:CARGO_PROFILE_RELEASE_DEBUG = "line-tables-only"` (the script says so when the PDB has no line tables). Scan **release** binaries: in a dev build intrinsics and small generics are not inlined, so their
   own copies show up as AVX functions; use `-NoIsa` / `--no-isa` for dev builds. `--isa-only` /
   `-IsaOnly` scans binaries without launching anything.

Outputs go to `target/smoke/` (`--out`): app log, screenshot, CLI and MCP transcripts.

The native window title is BashCutPro since #90 (`apps/filmcraft/src/main.rs`, one `with_title` hook after upstream's).
