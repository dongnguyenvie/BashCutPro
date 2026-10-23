<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="../bashcut/brand/wordmark/bashcutpro-wordmark-white.png">
    <img alt="BashCutPro" src="../bashcut/brand/wordmark/bashcutpro-wordmark.png" width="420">
  </picture>
</p>

# BashCutPro

> **Status: preview, still in development.** BashCutPro is not finished yet. For everyday editing use
> [BashCut](https://bashcut.birchapps.com), the stable app ([releases](https://github.com/dongnguyenvie/BashCut/releases)).
> BashCutPro releases are published as GitHub pre-releases.
>
> **Trạng thái: bản xem trước, vẫn đang phát triển.** Để dựng video hằng ngày, hãy dùng
> [BashCut](https://bashcut.birchapps.com/vi), bản ổn định ([các bản phát hành](https://github.com/dongnguyenvie/BashCut/releases)).

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

BashCutPro is under the same terms as FilmCraft: licensed under either of
[Apache License, Version 2.0](../LICENSE-APACHE) or [MIT license](../LICENSE-MIT), at your option. Every crate,
ours included, takes `license = "MIT OR Apache-2.0"` from the workspace.

FilmCraft's copyright (ArtCraft Team and the FilmCraft contributors) and the required notices stay in
[`LICENSE-MIT`](../LICENSE-MIT) and [`NOTICE`](../NOTICE), next to ours. BashCutPro's icon and wordmarks in
[`bashcut/brand/`](../bashcut/brand/) are MIT OR Apache-2.0 too ([`bashcut/brand/LICENSE.txt`](../bashcut/brand/LICENSE.txt));
every bundled asset is listed with its author and licence in [`ATTRIBUTION.md`](../ATTRIBUTION.md).
BashCutPro is not made, endorsed or supported by the ArtCraft team; the ArtCraft marks are not used in
its builds. More in [`bashcut/README.md`](../bashcut/README.md#license).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
