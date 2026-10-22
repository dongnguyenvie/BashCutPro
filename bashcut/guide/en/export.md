# Output profiles: the right export for each platform

An output profile is everything a platform wants from a video: size and frame rate, codec, bitrate from the
platform's own upload advice, loudness, captions (burned in or as a file) and the safe area its buttons cover.

| Profile | Shape | For |
|---|---|---|
| **TikTok**, **Reels**, **Shorts** | 1080×1920 vertical | short vertical videos |
| **YouTube** | 1920×1080 | landscape videos |
| **YouTube 4K** | 3840×2160 | 4K landscape |
| **Facebook square** | 1080×1080 | square posts |

## Export with a profile

The first-run setup turns each profile into an export preset named **BashCut** + the profile (for example
**BashCut TikTok**). If you skipped that step, open the palette (**Cmd+Shift+P**) and run **Compile Output Profiles to
Export Presets** once.

Then export as usual in FilmCraft (**File ▸ Export ▸ Media…**, the Export mode, the export queue or Quick Export) and
pick the **BashCut …** preset. That's all: size, bitrate and loudness are set for the platform.

In a batch, choose the profile under **Outputs** ([Batch](batch.md)). The palette also has **Export with Output Profile**
(press Tab to give the profile and the file to write).

## Your own profiles

A profile is a small JSON file. Put yours in `<data folder>/bashcut/profiles/` (the data folder is BashCutPro's:
`~/Library/Application Support/BashCutPro` on a Mac, `%APPDATA%\BashCutPro` on Windows, `~/.local/share/bashcutpro` on
Linux). A file with the same `id` as a built-in profile replaces it. Run **Compile Output Profiles to Export Presets**
again afterwards.
