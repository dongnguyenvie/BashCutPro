//! BashCutPro's engine commands (`bashcut.*`). Rules: `bashcut/extending-upstream.md` (add, never modify
//! upstream). This module is reached from upstream code by two inserted lines only: `pub mod bashcut;` in
//! `lib.rs` and `v.extend(crate::bashcut::commands());` in `commands::build()`.
//!
//! Each feature lives in its own file here and appends its specs in [`commands`]. Command ids start with
//! `bashcut.`; params strings state their units (seconds unless they say otherwise).

pub mod analysis;
pub mod batch;
#[cfg(test)]
mod batch_tests;
mod data;
pub mod errors;
pub mod hooks;
#[cfg(test)]
mod hooks_tests;
pub mod profiles;
#[cfg(test)]
mod profiles_tests;
pub mod summary;
#[cfg(test)]
mod summary_tests;
pub mod templates;
#[cfg(test)]
mod templates_tests;
#[cfg(test)]
mod tests;
pub mod voice;
#[cfg(test)]
mod voice_tests;
pub mod word_captions;
#[cfg(test)]
mod word_captions_tests;

use crate::commands::CommandSpec;

/// Every `bashcut.*` engine command, in menu / listing order.
pub fn commands() -> Vec<CommandSpec> {
    // one line per feature file, appended at the end: `<file>::commands(),`
    let groups: Vec<Vec<CommandSpec>> = vec![
        templates::commands(),
        profiles::commands(),
        voice::commands(),
        analysis::commands(),
        batch::commands(),
        hooks::commands(),
        summary::commands(),
        word_captions::commands(),
    ];
    // typed errors (`notFound: …`) and a units note on every command (errors.rs)
    errors::wrap(groups.into_iter().flatten().collect())
}
