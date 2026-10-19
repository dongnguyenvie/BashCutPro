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
   | peaks | `hard_limiter` | max −1 dB |

2. Matches loudness with `essentialSound.autoMatch` (BS.1770, measured after the chain): `target` LUFS, else the
   output profile's `loudnessLufs` (`profile`, [output-profiles.md](output-profiles.md)), else the dialogue target
   from the preferences. Untyped clips get the Dialogue audio type first.
3. Folds every step into one undo step `Voice Polish`. `amount` 0 changes nothing.
4. `savePreset`: saves the chain as the effect preset `BashCut Voice Polish NN%` (`presets.save`) to apply by hand.

Polishing a clip twice appends a second chain (no de-duplication yet).
