# BashCutPro brand

BashCutPro's own app icon, mark and wordmarks. They replace the ArtCraft marks in our builds, as
`docs/brand/LICENSE-brand.txt` requires for a modified version of FilmCraft. Upstream's files
(`docs/brand/`, `assets/app-icon/`) stay in the repository untouched so upstream merges stay clean;
the places that now point here are listed in [`../rebrand.md`](../rebrand.md).

## Artwork

- **Mark:** BashCut's: a shell prompt (`>_`) above a four-clip timeline with a playhead, on a dark
  navy tile (512-unit viewBox, `rx=114`). Transcribed from BashCut's
  `BashCut/Views/BashCutLogo.swift` with the same geometry and colours.
- **App icon:** the mark plus a blue-to-violet **PRO** pill top right, so BashCut and BashCutPro
  differ in the Dock.
- **Wordmark:** the mark (no badge) followed by "BashCut" in ink and "Pro" in a gradient, Inter Bold
  as outlines. Two inks: dark (`#0b1220`, for light themes) and white (`#f5f7fa`, for dark themes).

| Colour | Hex |
|---|---|
| Cyan | `#3dccff` |
| Blue | `#296bff` |
| Violet | `#7a42f0` |
| Tile | `#121a2b` → `#05080f` |

## Files

| File | What |
|---|---|
| `app-icon/bashcutpro.svg` | canonical app icon |
| `app-icon/bashcutpro-1024.png`, `-512.png` | full-bleed renders |
| `app-icon/bashcutpro-macos-512.png` | runtime Dock icon on macOS (Apple grid; `apps/filmcraft/src/main.rs`) |
| `app-icon/bashcutpro.icns` | macOS bundle icon (`packaging/macos/package.sh`) |
| `app-icon/bashcutpro.ico` | Windows icon, 16–256 px (`apps/filmcraft/build.rs`, `packaging/windows/package.ps1`) |
| `app-icon/hicolor/…/ai.storyteller.filmcraft.png`, `.svg` | Linux icon theme (named after the desktop file id, still upstream's) |
| `wordmark/bashcutpro-wordmark.svg` / `.png` | wordmark, dark ink (`crates/ui-egui/src/brand.rs`) |
| `wordmark/bashcutpro-wordmark-white.svg` / `.png` | wordmark, white ink |

## Regenerate

```
python3 bashcut/brand/generate.py   # SVGs (needs fontTools: pip install fonttools)
bashcut/brand/render.sh             # PNG, .icns, .ico (needs resvg or rsvg-convert; iconutil on macOS)
```

Licence: [LICENSE.txt](LICENSE.txt) (MIT OR Apache-2.0). Every file has a `.attribution` sidecar and
a row in `ATTRIBUTION.md` (section "BashCutPro assets"); `cargo xtask assets` checks both.
