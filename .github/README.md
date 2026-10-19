<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="../bashcut/brand/wordmark/bashcutpro-wordmark-white.png">
    <img alt="BashCutPro" src="../bashcut/brand/wordmark/bashcutpro-wordmark.png" width="420">
  </picture>
</p>

# BashCutPro

BashCutPro is a cross-platform, pure-Rust non-linear video editor built for editing with AI agents.
It is based on [FilmCraft](https://github.com/storytold/filmcraft) by the ArtCraft team, and brings
what [BashCut](https://bashcut.birchapps.com) (our macOS editor) does for agents (editing skills,
plugins for text to speech, captions and image generation, the edit workflow) to FilmCraft's engine.
It runs on macOS, Windows and Linux.

BashCutPro is a fork that keeps merging upstream FilmCraft. Our own work lives in [`bashcut/`](../bashcut/)
and in new `bashcut`-prefixed crates; the rest of the repository is FilmCraft's, and its
[README](../README.md), [`AGENTS.md`](../AGENTS.md) and [`docs/`](../docs/) still apply.

## Start here

- [`bashcut/README.md`](../bashcut/README.md): what the fork is, the related repositories, the direction.
- [`bashcut/decisions.md`](../bashcut/decisions.md) and [`bashcut/status.md`](../bashcut/status.md): decisions made and where the work stands.
- [`bashcut/rebrand.md`](../bashcut/rebrand.md): where BashCutPro's name, icon and links replace FilmCraft's.
- [`UPSTREAM.md`](../UPSTREAM.md): how we merge FilmCraft.

## Build and run

```sh
cargo run -p filmcraft                      # the desktop app
cargo run -p filmcraft -- --control 9876    # with the JSON-lines control server (docs/control-protocol.md)
cargo run -p filmcraft-cli -- mcp           # MCP server for agents (docs/agents.md)
```

Crates, binaries, command ids and file formats (`.fcproj`, `.fcgt`) keep their FilmCraft names, so
FilmCraft's documentation, projects and tools work unchanged. Before a commit, run the quality gates:
`cargo xtask ci`.

## Licence

MIT OR Apache-2.0, like FilmCraft ([`LICENSE-MIT`](../LICENSE-MIT), [`LICENSE-APACHE`](../LICENSE-APACHE),
[`NOTICE`](../NOTICE)). BashCutPro's icon and wordmarks are in [`bashcut/brand/`](../bashcut/brand/).
BashCutPro is not made, endorsed or supported by the ArtCraft team; the ArtCraft marks are not used in
its builds.
