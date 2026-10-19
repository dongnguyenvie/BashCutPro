//! BashCutPro's engine commands (`bashcut.*`). Rules: `bashcut/extending-upstream.md` (add, never modify
//! upstream). This module is reached from upstream code by two inserted lines only: `pub mod bashcut;` in
//! `lib.rs` and `v.extend(crate::bashcut::commands());` in `commands::build()`.
//!
//! Each feature lives in its own file here and appends its specs in [`commands`]. Command ids start with
//! `bashcut.`; params strings state their units (seconds unless they say otherwise).

pub mod analysis;
mod data;
pub mod profiles;
#[cfg(test)]
mod profiles_tests;
pub mod templates;
#[cfg(test)]
mod templates_tests;
#[cfg(test)]
mod tests;
pub mod voice;
#[cfg(test)]
mod voice_tests;

use crate::commands::CommandSpec;

/// Every `bashcut.*` engine command, in menu / listing order.
pub fn commands() -> Vec<CommandSpec> {
    // one line per feature file, appended at the end: `<file>::commands(),`
    let groups: Vec<Vec<CommandSpec>> = vec![templates::commands(), profiles::commands(), voice::commands(), analysis::commands()];
    groups.into_iter().flatten().collect()
}
