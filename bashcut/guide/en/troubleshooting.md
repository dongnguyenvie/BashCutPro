# Troubleshooting

## First: run the health check

Open a terminal and run `bashcut-doctor`. It checks the editor's command-line engine, the agent kit, each plugin's
Python, downloaded models (size and checksum), Claude Code and Codex, free disk space and memory, and says how to fix
each problem. `bashcut-doctor --updates` also tells you when plugins, the kit or BashCutPro have a newer version (it
never installs anything by itself). Batches run it before they start.

## Common problems

| You see | What to do |
|---|---|
| Plugins says *Control channel is off*, a project action's **Run** is greyed out with *needs the control channel*, or an action says it *needs the open project* | The channel is on by default, so it was turned off: tick **Allow AI agents to control BashCutPro** in the Agent section (or press **Turn on**) and restart BashCutPro; if `BASHCUT_CONTROL=0` is set, unset it. `--control 9876` also turns it on for one start. Searches, downloads and other actions that do not edit the project work without it. |
| A plugin will not run, or says *Python 3 was not found* | It needs Python 3.9 or newer (`bashcut-doctor` names the plugin). Install Python and run the action again. |
| A run says *The BashCutPro engine (bashcutpro or filmcraft-cli) was not found next to the app* | Plugins edit the project through the engine beside the app. Reinstall BashCutPro, or build it as [Getting started](getting-started.md) shows, or point `BASHCUTPRO_CLI` at it. |
| A run says **Needs the agent** | The plugin could not finish alone (for example a translation without an API key). **Send to agent** in Runs, or add the key in the plugin's **Settings** and run it again. |
| The Agent section does nothing, or says *Claude Code was not found* | Install Claude Code and sign in (`claude` in a terminal). Save the project first. |
| A batch item **needs attention** | See [Fixing an item that needs a fix](batch.md#fixing-an-item-that-needs-a-fix). An item stopped in the middle of a step (the computer slept or crashed) also lands here: open it, check it, **Retry**. |
| A batch waits and does not start new items | It is short of memory or disk space; it goes on by itself when there is room. Free some disk space, or lower **At once**. |
| Library ▸ Online shows *Licence unknown* | The source gave no licence. Do not publish it until you know. |
| The rail is gone | **Window ▸ BashCutPro ▸ Show Rail**. It also hides in Import and Export mode. |
| A shortcut does nothing | Check **Edit ▸ Keyboard Shortcuts**; the palette is **Cmd+Shift+P** (Cmd+K is FilmCraft's Add Edit). |

## Where BashCutPro keeps things

In BashCutPro's own data folder (`~/Library/Application Support/BashCutPro` on a Mac, `%APPDATA%\BashCutPro` on
Windows, `~/.local/share/bashcutpro` on Linux; a portable install uses the `data` folder next to the app). It is not
FilmCraft's folder, so BashCutPro and FilmCraft can be installed side by side without sharing settings. On its first
launch BashCutPro copies your FilmCraft settings, recent projects, export presets, profiles, batch presets and
plugins once (the list is in `bashcut/imported-from-filmcraft.txt`); speech and voice models are not copied, they
are used from FilmCraft's folder until BashCutPro has its own `models` folder:

- `bashcut/`: your profiles, templates, looks, batch presets, and the side panel's settings;
- `bashcut-plugins/`: installed plugins and their downloads;
- next to each project, `<project>.fcproj.bashcut/`: agent conversations, the edit plan and plugin results.

A project made in BashCutPro still opens in plain FilmCraft; it simply ignores these extras.
