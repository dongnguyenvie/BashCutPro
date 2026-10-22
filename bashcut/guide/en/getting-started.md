# Getting started

## 1. Install

Install BashCutPro from its package for your system (macOS `.app`, Windows installer or portable zip, Linux package).
Every package also carries the command line `bashcutpro` (short name `bcpro`; it runs the editor's engine
`filmcraft-cli`, which sits beside it) and three helper programs: `bashcut-doctor` (checks your setup), `bashcut-batch`
(runs batches) and `bashcut-mcp` (lets an AI agent reach the open window), plus a first-run setup script.

### No package yet? Build it from source

There is no published package yet, so for now you build BashCutPro yourself. It takes about ten minutes on a recent
Mac, and needs about 10 GB of free disk space. These steps are for macOS; on Linux and Windows the `cargo` command is
the same.

1. Install Apple's command line tools if `git --version` does not answer yet: `xcode-select --install`.
2. Install Rust (version 1.95 or newer):
   `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`, then open a new terminal so `cargo` is found
   (`rustc --version` checks it).
3. Get the code. The repositories are private for now, so your GitHub account needs access:
   `gh repo clone dongnguyenvie/BashCutPro && cd BashCutPro`.
4. Build the app and its command-line tools, with speech to text:

   ```sh
   cargo build -p filmcraft -p filmcraft-cli -p bashcut-cli -p bashcut-doctor -p bashcut-batch-cli -p bashcut-mcp \
     --features filmcraft/whisper,filmcraft-cli/whisper
   ```

5. Make the tools reachable in this terminal: `export PATH="$PWD/target/debug:$PATH"` (add that line, with the full
   path, to `~/.zshrc` to keep it). `bashcutpro --version` then answers `filmcraft-cli 0.9.0`, which is the engine
   it runs.

From a source build, the first-run setup below is `bash bashcut/packaging/share/bashcut-setup.sh`, and you open the app
with `target/debug/filmcraft --control 9876`.

To make the macOS app and its DMG instead, run `bash packaging/macos/package.sh --arch aarch64` (use `x86_64` on an
Intel Mac). This does a full release build, so it takes longer. The result goes to `dist/release/`. It is signed only for this
Mac: the copy opens here, but other Macs refuse it until it is signed and notarised with a Developer ID.

## 2. Run the first-run setup (optional, once)

The setup prepares BashCutPro's own files. Every step can be skipped.

| System | Where the script is | Run |
|---|---|---|
| macOS | inside the app: `BashCutPro.app/Contents/Resources/bashcut/` | `bash bashcut-setup.sh` |
| Windows | the install folder | `powershell -File bashcut-setup.ps1` |
| Linux | `/usr/share/bashcutpro/` | `bash bashcut-setup.sh` |

It makes the folders for your profiles, templates and looks, writes the look pack as `.cube` LUT files, turns the
output profiles into export presets (named **BashCut TikTok**, **BashCut YouTube**…), offers to install the agent kit
when Claude Code or Codex is on the computer, and ends with a health check (`bashcut-doctor`). Add `--yes` to accept
every step, or `--skip-looks`, `--skip-profiles`, `--skip-kit`, `--skip-doctor` to leave one out.

**The speech model.** Captions, the batch presets and the agent's rough cut start by turning speech into text, which
needs a speech model on this computer. Download it once: `bashcutpro exec transcript.downloadModel` gets
**whisper-base** (about 290 MB), the default. `bashcut-doctor` then shows `[ok] speech`. Until then, transcribing stops
with *the speech model `whisper-base` is not downloaded*. The smaller **whisper-tiny** (about 150 MB:
`bashcutpro exec transcript.downloadModel model=whisper-tiny`) is faster but less accurate. To make it the one used,
choose it in **Settings ▸ Media Analysis & Transcription ▸ Speech model**.

## 3. Open the app

You see FilmCraft's editor with one addition: a narrow **rail** on the right edge. Each icon opens a section in a
side panel: **Agent**, **Batch**, **Looks**, **Library**, **Plugins**, **Runs** ([The rail](rail.md)). Click the same icon
again to close it. Drag the panel's left edge to make it wider (240–720 px). The rail hides in Import and Export
mode, and **Window ▸ BashCutPro ▸ Show Rail** turns it off and on.

![The BashCutPro window: the rail on the right edge, the Batch section open beside it](../img/window.png)

## 4. Find anything: the command palette

Press **Cmd+Shift+P**. Type a few letters of what you want: a command ("razor", "export"), a look ("teal"), an output
profile ("tiktok"), a plugin action, a template or a recipe ("travel vlog"). **↑ ↓** choose, **Enter** runs, **Esc**
closes. Things you use often move to the top. Some commands need values: **Tab** (or Enter on such a command) opens a
small editor with them; change the values and press **Cmd+Enter**. Everything the palette runs can be undone with
**Cmd+Z** like any edit.

![The command palette searching for "razor"](../img/palette.png)

(**Cmd+K** stays FilmCraft's *Add Edit*, so the palette has its own key. You can change any shortcut in
**Edit ▸ Keyboard Shortcuts**.)

## 5. Vietnamese

BashCutPro's own panels can be shown in Vietnamese: choose **Window ▸ BashCutPro ▸ Language ▸ Tiếng Việt**. The panels
switch at once and stay that way the next time you open the app. **English** keeps them in English whatever the app's
language is; **Follow App** (the default) uses the app's interface language. The check mark shows the current choice.

Next: [The rail](rail.md).
