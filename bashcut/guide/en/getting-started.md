# Getting started

## 1. Install

Install BashCutPro from its package for your system (macOS `.app`, Windows installer or portable zip, Linux package).
Every package also carries three helper programs: `bashcut-doctor` (checks your setup), `bashcut-batch` (runs batches)
and `bashcut-mcp` (lets an AI agent reach the open window), plus a first-run setup script.

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

## 3. Open the app

You see FilmCraft's editor with one addition: a narrow **rail** on the right edge. Each icon opens a section in a
side panel: **Agent**, **Batch**, **Looks**, **Library**, **Plugins**, **Runs** ([The rail](rail.md)). Click the same icon
again to close it. Drag the panel's left edge to make it wider (240–720 px). The rail hides in Import and Export
mode, and **Window ▸ BashCutPro ▸ Show Rail** turns it off and on.

## 4. Find anything: the command palette

Press **Cmd+Shift+P**. Type a few letters of what you want: a command ("razor", "export"), a look ("teal"), an output
profile ("tiktok"), a plugin action, a template or a recipe ("travel vlog"). **↑ ↓** choose, **Enter** runs, **Esc**
closes. Things you use often move to the top. Some commands need values: **Tab** (or Enter on such a command) opens a
small editor with them; change the values and press **Cmd+Enter**. Everything the palette runs can be undone with
**Cmd+Z** like any edit.

(**Cmd+K** stays FilmCraft's *Add Edit*, so the palette has its own key. You can change any shortcut in
**Edit ▸ Keyboard Shortcuts**.)

## 5. Vietnamese

BashCutPro's own panels can be shown in Vietnamese: open the palette, type **BashCutPro Language**, press **Tab**,
replace `{}` with `{"language": "vi"}` and press **Cmd+Enter**. Running it with `{}` goes back to the app's language.

Next: [The rail](rail.md).
