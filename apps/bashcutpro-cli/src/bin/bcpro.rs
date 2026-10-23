//! `bcpro`: the short alias of `bashcutpro-cli` (#154), the same generated program.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

#[path = "../bridge_default.rs"]
mod bridge_default;

include!(concat!(env!("OUT_DIR"), "/main.rs"));
