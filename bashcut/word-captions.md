# Word-by-word caption styles (#49): design note

Status: no-hook route implemented in `crates/engine/src/bashcut/word_captions.rs` (tests in `word_captions_tests.rs`);
the box style waits for the owner (#49).

The spoken word highlighted, the most requested short-form look. FilmCraft has no word styling for captions
([skill-needs.md](skill-needs.md) #4). Rules: [extending-upstream.md](extending-upstream.md); `crates/render/src/lib.rs`
is a hot spot (40–54 upstream changes in 30 days), so the no-hook route comes first.

## What FilmCraft already has

- **Caption tracks** (`crates/project/src/caption.rs`): a caption is `{start, duration, text}`; its look is the
  *track's* `CaptionStyle` (font, size, colour, outline, background, alignment, anchor). There are no per-word
  styles and nothing per word in the caption renderer, so styling captions themselves would need a render hook.
- **Graphic text layers** (`crates/project/src/graphic.rs`, `graphic_design.rs`): a text layer has per-character
  **style runs** (`StyleRun {start, end, style: CharStyle}`: font, font style, size, fill colour with alpha, faux bold /
  italic, underline, tracking, baseline shift, caps). The renderer lays them out with the rest of the text
  (`filmcraft_render::graphic_clip::text_layout`), wrapping included, so a styled word stays in reading order across
  wrapped lines with no work on our side.
- **Commands that connect the two**: `graphics.upgradeCaption {captions}` turns captions into graphic clips with one
  text layer that copies the track style (font, size, colour, alignment, anchor, outline, background);
  `graphics.setCharStyle {clip, layer, start, end, style}` sets a run (character offsets);
  `captions.split {caption, time}` splits a caption keeping its text on both halves; `transcript.inspect` gives the
  sequence's words with start / end ticks.

## The no-hook route

`bashcut.captions.wordStyle {captions?, style}` builds the look from those commands, as one undo step:

1. For each caption, the words of its text (whitespace-separated, tags like `<i>` stripped as `upgradeCaption` does)
   and their times: the transcript words that fall in the caption when their count matches, else the caption's
   duration shared by character count.
2. Split the caption at each word's start (`captions.split`, frame-snapped; a word shorter than the minimum
   caption length joins the previous segment).
3. Upgrade the segments to graphics (`graphics.upgradeCaption`): one graphic clip per segment, same text, same place.
4. In segment *k*, style word *k* (`graphics.setCharStyle`): highlight colour, **pop** as a larger size (and optional
   faux bold), and with **dim** the words after it at the base colour with lower alpha.
5. Fold every step into one undo entry (`History` is public).

Style: `{"highlight":"#ffd400","pop":1.0–1.6,"bold":bool,"dim":0–1 (alpha of upcoming words, 1 = off),
"preset":"karaoke|pop|clean"}`.

Data and compatibility: nothing new in `.fcproj`; the result is ordinary graphic clips and text styles, which open in
upstream FilmCraft as they are. The words stay editable in Essential Graphics. Re-running on graphics is not
supported (the command takes captions; undo restores them).

## Limits of this route, and what a hook would add

| Look | No-hook route | Notes |
|---|---|---|
| Highlight colour | yes (`CharStyle.fill`) | exact |
| Pop / scale | yes, as a larger size of the word | the line reflows by the word's growth; keep `pop` ≤ 1.25 for long lines. A true scale-in-place without reflow would need a render hook |
| Upcoming words dimmed | yes (fill alpha) | exact |
| Box behind the word | **not in v1** | `CharStyle` has no background. No-hook option for v2: a rectangle shape layer under the text in each segment, sized from the word's glyphs in `text_layout` (glyph `x`, `cluster`, `size`) and the layer transform; engine-side only. Owner question left on #49 |
| Smooth animation inside a word | no | segments switch at word starts (frame accurate); per-frame tweening would need keyframes on run styles, which FilmCraft does not keep |

**Conclusion:** highlight, pop and dim need no upstream hook and no render change; implement them now in
`crates/engine/src/bashcut/word_captions.rs`. The box style is the open question (v2 shape-layer route, still no hook).

## Test (build pass)

A caption on a black sequence, words timed by a transcript or by the even split, `bashcut.captions.wordStyle` with a
red highlight: render the program at three times (one per word) and check that the highlighted word's pixels
(left / middle / right third of the text box) are red only in its own segment; hostile params; one undo step.
Manual: a phone-size check of the three presets.
