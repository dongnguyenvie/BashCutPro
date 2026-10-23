# BashCutPro engine commands (`bashcut.*`)

Our own engine commands (29), in listing order; upstream's are in [commands.md](commands.md). Generated from
`bashcutpro commands bashcut --json` (debug build, `main` 2026-10-23); `bashcutpro describe <id>` prints one. Every
one has no menu and no shortcut, returns typed errors (`notFound: …`) and states its units in `params`. Regenerate
after adding or renaming one (RC12; `bashcut::tests::bashcut_commands_doc_matches_the_engine` fails until you do):

```bash
cargo build -p bashcutpro-cli
target/debug/bashcutpro commands bashcut --json > /tmp/bashcut-commands.json   # then rebuild this table
```

Namespaces: `template` 3, `profiles` 4, `audio` 3, `review` 2, `batch` 1, `hooks` 2, `media` 1, `timeline` 1, `captions` 2, `looks` 7, `transcript` 3.

| Id | Label | Params |
|---|---|---|
| `bashcut.template.save` | Save as Template… | <code>{"path":str (bundle folder) \| "name":str (in &lt;data dir&gt;/bashcut/templates),"slots":[id \| {"item":id,"name":str?}],"title":str?,"poster":bool=true,"overwrite":bool=false,"dir":str?} (units: none (paths, names and item ids))</code> |
| `bashcut.template.instantiate` | New Project from Template… | <code>{"template":str (folder or name),"fills":[{"slot":str,"file":str}],"saveAs":str?,"dir":str?} (units: none (paths and names))</code> |
| `bashcut.template.list` | List Templates | <code>{"dir":str?} (units: none)</code> |
| `bashcut.profiles.list` | List Output Profiles | <code>{"seconds":f64? (estimate; default the active sequence),"dir":str?}</code> |
| `bashcut.profiles.get` | Get Output Profile | <code>{"profile":str,"seconds":f64?,"dir":str?}</code> |
| `bashcut.profiles.compile` | Compile Output Profiles to Export Presets | <code>{"profile":str? (default every profile),"dir":str?} (units: none)</code> |
| `bashcut.profiles.apply` | Export with Output Profile | <code>{"profile":str,"path":str?,"queue":bool=false,"wait":bool=false,"sequence":id?,"range":str?,"hooks":bool=true,"dir":str?}  (no path: compile only, for filmcraft-cli export --preset) (units: none here; the profile sets pixels, kbit/s and LUFS)</code> |
| `bashcut.audio.polish` | Polish Voice | <code>{"clips":[id]? (default the selection),"amount":0..1=0.5,"target":lufs?,"profile":str? (its loudness when no target),"loudness":bool=true,"savePreset":bool=false,"dir":str?}</code> |
| `bashcut.audio.loudness` | Measure Loudness | <code>{"clip":id?,"track":id\|"A1"?,"range":{"start":seconds?,"end":seconds?}?,"series":bool?} (no clip / track = the sequence mix; result in LUFS, LU, dBTP, seconds)</code> |
| `bashcut.audio.beats` | Detect Beats | <code>{"clip":id?,"writeMarkers":bool?} (default: the selected audio clip; result in sequence seconds and BPM)</code> |
| `bashcut.review.pictureChecks` | Picture Checks | <code>{"range":{"start":seconds?,"end":seconds?}?,"samplesPerSecond":0.5..60=5} (black, frozen, long-still and flash stretches; result in seconds)</code> |
| `bashcut.batch` | Run Batch | <code>{"commands":[{"id":str,"params":{}?}] (at most 10000),"atomic":bool=true,"label":str?} (one undo step; atomic: an error restores the project and names the failing step; times inside each step's params use that command's units)</code> |
| `bashcut.hooks.list` | List Plugin Hooks | <code>{"event":"batch.beforeItem\|batch.afterItem\|import.after\|export.before"?,"pluginsDir":str?} (units: none)</code> |
| `bashcut.hooks.run` | Run Plugin Hooks | <code>{"event":"batch.beforeItem\|batch.afterItem\|import.after\|export.before","context":{}?,"hooks":bool=true,"pluginsDir":str?}  (the batch runner fires the batch.* events) (units: none)</code> |
| `bashcut.media.import` | Import (with Plugin Hooks) | <code>{"paths":[str], …file.import params, "hooks":bool=true,"pluginsDir":str?}  (file.import, then the import.after hooks) (units: none)</code> |
| `bashcut.timeline.summary` | Timeline Summary | <code>{"sequence":id? (default the active sequence)} (tracks and clips with ids; start / end / in / duration in seconds, only fields that differ from a plain clip)</code> |
| `bashcut.captions.wordStyle` | Word-by-Word Caption Style | <code>{"captions":[id]? (default: the selected captions, else every caption of the first track),"style":{"preset":"karaoke\|pop\|clean"?,"highlight":"#rrggbb"?,"pop":1..1.6?,"bold":bool?,"dim":0..1? (alpha of the upcoming words; 1 = off)}?,"useTranscript":bool=true} (units: none)</code> |
| `bashcut.review.run` | Review Edit | <code>{"profile":str? (loudness target and caption safe area from bashcut.profiles.*; default -14 LUFS / -1 dBTP, no safe area),"checks":["sound"\|"picture"\|"captions"\|"edit"]?,"range":{"start":seconds?,"end":seconds?}?,"writeMarkers":bool=true,"samplesPerSecond":0.5..60=5,"sequence":id? (must be the active one)} (findings in seconds; markers replace the previous review's)</code> |
| `bashcut.looks.list` | List Looks | <code>{"tags":[str]\|str? (any of; e.g. "talking-head","travel","food"),"query":str?} (units: none)</code> |
| `bashcut.looks.get` | Get Look | <code>{"look":str,"intensity":0..1.5=1?} (units: none)</code> |
| `bashcut.looks.apply` | Apply Look | <code>{"look":str,"intensity":0..1.5=1? (clamped to the look's safe range),"clips":[id]? (default the selection)} (units: none)</code> |
| `bashcut.looks.remove` | Remove Look | <code>{"clips":[id]? (default the selection)} (units: none)</code> |
| `bashcut.looks.thumbnail` | Look Thumbnail | <code>{"look":str,"intensity":0..1.5=1?,"width":n=160,"frame":bool=false (on the program frame at the playhead),"path":str? (PNG; default a cache file in the data dir)} (units: none)</code> |
| `bashcut.looks.export` | Export Looks as LUTs | <code>{"look":str?,"path":str? (.cube, with look),"dir":str? (the whole pack + looks.json),"intensity":0..1.5=1?,"size":2..65=33} (units: none)</code> |
| `bashcut.looks.check` | Check Looks | <code>{"look":str? (default every look)}  (clipped highlights, crushed shadows, skin hue shift at min / default / max) (units: none)</code> |
| `bashcut.transcript.generate` | Transcribe (Drop Doubtful Text) | <code>{…transcript.generate params, "keepDoubtful":bool=false,"minRepeats":n=3,"keepWordTimes":bool=false,"minSilenceSeconds":f=0.3}  (transcribes, moves word edges out of pauses measured on the audio (edgesMoved), then drops Whisper's repeated-text loops; an item with only loops reports noSpeech and keeps an empty transcript)</code> |
| `bashcut.transcript.clean` | Drop Doubtful Transcript Text | <code>{"items":[id]? (default every transcript a recogniser made),"minRepeats":n=3,"dryRun":bool=false}  (doubtful stretches: start / end in media seconds)</code> |
| `bashcut.captions.create` | Create Readable Captions from Transcript… | <code>{"maxChars":n=42 (characters a line),"lines":1\|2=2,"maxCps":f=20 (characters a second),"minSeconds":f=0.7,"maxSeconds":f=7,"breakSeconds":f=0.5 (a pause this long starts a cue),"fill":0..1=0.5 (break at a comma once the cue is this full),"gapFrames":n=2,"leadSeconds":f=0.15 (a cue may start this much before its first word),"keepFillers":bool=false,"trimHedges":bool=false,"format":str?,"name":str?="Captions","dryRun":bool=false}</code> |
| `bashcut.transcript.refineEdges` | Refine Word Edges from Audio | <code>{"items":[id]? (default every transcript a recogniser made),"minSilenceSeconds":f=0.3 (0.1..5),"dryRun":bool=false}  (moves word edges out of pauses measured on the audio; one undo step)</code> |
