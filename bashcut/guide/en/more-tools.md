# More tools

These are in the command palette (**Cmd+Shift+P**); type their name.

## Polish Voice

One knob for a clean voice. Select the clips with speech and run **Polish Voice**. It removes rumble and steady noise,
softens harsh "s" sounds, evens the level, catches peaks and matches the loudness for dialogue, all as one undo step.
Press **Tab** first to change `amount` (0 to 1, default 0.5) or to aim at an output profile's loudness
(`"profile": "tiktok"`).

## Word-by-Word Caption Style

The spoken word lights up as it is said, the look short videos use. After making captions from speech, run
**Word-by-Word Caption Style** and press **Tab** to set the style: `highlight` colour (for example `"#ffd400"`), `pop`
(how much bigger the current word gets, 1.0–1.6), `bold`, and `dim` (how faded the coming words are, 1 = off). It
turns the captions into editable titles, as one undo step.

## Templates

Make a project once, reuse it with new footage. **Save as Template…** keeps the project with *slots* (the clips,
music bed or logo to replace); **New Project from Template…** fills the slots with your new files and saves a new
project, keeping every cut, effect and caption position. Batches can start each item from a template.

## Review Edit

**Review Edit** checks the edit the way a careful editor would before export: loudness and peaks, black or frozen
frames, flashes, captions, cuts. Each problem becomes a **Review marker** on the timeline with what is wrong, and the
result says whether the edit is ready or **needs fix**. Agents run it before every export.

## Measurements

**Measure Loudness** (LUFS and true peak of the mix, a track or a clip), **Detect Beats** (tempo and beats of a music
clip; can add markers on the beats) and **Picture Checks** (black, frozen, long-still and flash stretches).
