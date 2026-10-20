# The agent: editing with Claude Code or Codex

BashCutPro can be edited by an AI agent that uses the same commands you do. It follows the agent kit's editing
skills (rough cut by quote, captions, colour by measurement, loudness for each platform, review before export…) and
stops at each stage for your approval.

![The Agent section](../img/agent.png)

## In the app: the Agent section

You need **Claude Code** installed and signed in on this computer (the `claude` command). If it is installed somewhere
unusual, set the environment variable `BASHCUT_CLAUDE` to its path.

1. **Save the project** first: conversations and the edit plan are kept next to it (in `<project>.fcproj.bashcut/`).
2. Open **Agent** on the rail (**Cmd+Alt+Shift+A**). Type what you want ("cut the ums, keep it under a minute, add
   Vietnamese captions") and press **Enter** (**Shift+Enter** for a new line).
3. The agent's replies stream in; the commands it runs are folded into one line each. Every edit it makes is a normal
   undo step.
4. At each stage (brief, plan, rough cut, draft) it shows a **gate** card with what it did and the measurements behind
   it: **Approve** to go on, or write a note and **Ask for changes**.
5. **Stop** ends its turn at once; your project stays as it is.

**New** starts a new conversation; earlier ones are listed and can be opened again.

## Codex

Codex works as an outside agent (below) and in batch pipelines, whose agent steps can use `codex` instead of
Claude Code. The in-app Agent section uses Claude Code for now.

## An agent in your terminal

Claude Code or Codex in a terminal can drive the open window too:

1. Install the agent kit (the first-run setup offers it), which registers BashCutPro's tools with the agent.
2. Start BashCutPro with its control channel: add `--control 9876` when starting it (on a Mac:
   `open -a BashCutPro --args --control 9876`), or set `FILMCRAFT_CONTROL_PORT=9876` before starting it. The window
   then tells agents where to find it.
3. Ask the agent in the terminal; it connects to the open window by itself. With no window open, it works on the
   project file directly, and it refuses to open a project the window already has open.

Agents can only write inside the project's folder (and folders you allow), so a mistake cannot overwrite your other
files.
