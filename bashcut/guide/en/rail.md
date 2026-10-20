# The rail

The rail is the strip of icons on the right edge of the window. Each icon opens one section in the side panel; click
it again to close the panel. The same sections are in **Window ▸ BashCutPro**.

![The BashCutPro window: the rail on the right edge, the Batch section open beside it](../img/window.png)

| Section | What it is for | Guide |
|---|---|---|
| **Agent** | Chat with an AI editor (Claude Code) about this project; it edits, you approve each stage | [The agent](agent.md) |
| **Batch** | Run the same steps on many files: each file becomes its own project and its own exports | [Batch](batch.md) |
| **Looks** | 26 colour looks, each with a one-line purpose; preview and apply to the selected clips | [Looks](looks.md) |
| **Library** | **Mine**: your saved pieces. **Online**: open music, sound effects, stock photos and video, icons, fonts | [Library](library.md) |
| **Plugins** | Install plugins (Vietnamese text to speech, caption translation, AI images, online resources…), set their keys, run their actions | below |
| **Runs** | Every plugin run and agent turn, with progress, result and log | below |

**Pop out.** The square button in a section's header moves it into its own window (handy on a second screen);
**Dock back** returns it to the side panel. **Cmd+Alt+Shift+A** jumps to the Agent section.

## The command palette

**Cmd+Shift+P** (or **Window ▸ BashCutPro ▸ Command Palette…**) finds anything by a few letters: commands, looks,
output profiles, plugin actions, templates and recipes. **↑ ↓** choose, **Enter** runs, **Esc** closes; **Tab** opens
the values of a command that takes some, with a line above saying which keys it accepts (`?` marks the optional ones).
See [Getting started](getting-started.md#4-find-anything-the-command-palette).

![The command palette searching for "razor"](../img/palette.png)

## Plugins

Plugins are small programs that run beside the app and bring their results in as ordinary media, captions or edits.

![The Plugins section with two plugins installed](../img/plugins.png)

1. Open **Plugins** and press **Check registry** to see what is available.
2. **Install** the ones you want. Most need Python 3.9 or newer on the computer.
3. Open a plugin's **Settings** to fill in its options. API keys are kept in your system's key store (Keychain,
   Windows Credential Manager…), never in a file.
4. Each **action** has a small form; **Run** starts it. Actions that edit the open project need the app to have been
   started with its control channel (see [Troubleshooting](troubleshooting.md)).

Removing a plugin also forgets its stored keys.

## Runs

![The Runs section before anything has run](../img/runs.png)

Runs lists everything that works in the background: plugin actions, installs, agent turns. Each row shows progress,
time, the result's message, **Log** (what the plugin printed) and **Cancel** while it runs. **Clear finished** tidies
the list. When a plugin needs help (for example a translation without an API key) its run says **Needs the agent**:
**Send to agent** hands the job to the Agent section, which finishes it.

You never have to wait for a run: keep editing while it works.
