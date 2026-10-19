# Upstream

BashCutPro is based on **FilmCraft**.

- Source: https://github.com/storytold/filmcraft
- License: dual MIT / Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `ATTRIBUTION.md` kept as-is)
- Remote: `upstream` (fetch only; push disabled)

## Sync log

Newest first. Add a row after every merge from `upstream/main`.

| Date (sync) | Upstream commit | Upstream commit date | Note |
|---|---|---|---|
| 2026-10-20 | `105c09bd80e2c86ec7464a2c59a2501eb89ffc5c` | 2026-10-19 | Initial import (latest tag v0.8.0), PR #2452 merge |

## How to sync

```bash
git fetch upstream
git merge upstream/main
git log -1 --format='%H %cs' upstream/main   # values for the new row above
```
