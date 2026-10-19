# One-knob voice polish (#48)

Code: `crates/engine/src/bashcut/voice.rs`, tests in `voice_tests.rs`. Built on upstream commands only.

`bashcut.audio.polish {clips?, amount 0–1 = 0.5, target?, profile?, loudness = true, savePreset = false}`:

1. Appends one ordered chain of FilmCraft's audio effects to each audio clip (video clips use their linked audio),
   every stage scaled by `amount` (`a`):

   | Stage | Effect id | Parameters |
   |---|---|---|
   | rumble | `highpass` | cutoff 40 + 60·a Hz |
   | steady noise | `denoise` | amount 60·a % |
   | sibilance | `deesser` | 6.5 kHz, threshold −12 − 12·a dB, reduction 10·a dB |
   | level | `single_band_compressor` | threshold −10 − 14·a dB, ratio 1 + 3·a, attack 10 ms, release 120 ms |
   | makeup | `amplify` | gain 0 dB, set by step 2 |
   | peaks | `hard_limiter` | max = profile true peak − 0.5 dB (−1.5 dB by default), input boost set by step 2 |

2. Brings the clip to the loudness target (BS.1770, measured after the chain): `target` LUFS, else the output
   profile's `loudnessLufs` (`profile`, [output-profiles.md](output-profiles.md)), else the dialogue target from the
   preferences. Upstream's clip gain (what `essentialSound.autoMatch` writes) plays *after* the effects, so the gain
   goes in before the limiter: the limiter's input boost (up to +30 dB), then `amplify` (up to +24 dB), measured and
   set again for up to 8 passes, aiming 0.3 LU over the target. `essentialSound.autoMatch` then trims the rest down
   (a cut after the limiter can't clip). A clip that can't reach the target is reported with `short` and left
   un-trimmed. Untyped clips get the Dialogue audio type first. Build pass (#135): before this, a quiet take raised
   ~22 dB by Auto-Match alone reached the target at +3 dBTP with clipped samples.
3. Folds every step into one undo step `Voice Polish`. `amount` 0 changes nothing.
4. `savePreset`: saves the chain as the effect preset `BashCut Voice Polish NN%` (`presets.save`) to apply by hand.

Polishing a clip again replaces the polish chain at the end of its effects instead of stacking a second one.
