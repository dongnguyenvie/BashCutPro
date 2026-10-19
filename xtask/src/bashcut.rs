//! Layers of BashCutPro's own crates (fork-only file; see `bashcut/in-app-ui.md` §2).
//!
//! `layer_of` in `main.rs` asks this table first (one inserted line), so upstream's `LAYERS` table
//! is never edited. Names are full package names: ours carry no `filmcraft-` prefix.

/// (package name, layer).
const LAYERS: &[(&str, u8)] = &[
    // read-only measurements; deps only on L0–L1 crates, so the engine (L4) may use it
    ("bashcut-analysis", 3),
    // no workspace deps; only ui-egui (L5) and our apps use them
    ("bashcut-plugin-host", 4),
    ("bashcut-agent", 4),
    ("bashcut-batch", 4),
    // apps
    ("bashcut-batch-cli", 6),
    ("bashcut-doctor", 6),
];

/// The layer of one of our crates, or `None` for any other crate.
pub fn layer_of(name: &str) -> Option<u8> {
    LAYERS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

/// The hook in `main.rs`'s `layer_of`: returns our crate's layer early. A macro so the hook stays one
/// line after `cargo fmt`.
macro_rules! our_layer_first {
    ($name:expr) => {
        if let Some(l) = $crate::bashcut::layer_of($name) {
            return Some(l);
        }
    };
}
pub(crate) use our_layer_first;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_crates_have_layers_and_others_do_not() {
        assert_eq!(layer_of("bashcut-analysis"), Some(3));
        assert_eq!(layer_of("bashcut-plugin-host"), Some(4));
        assert_eq!(layer_of("filmcraft-engine"), None);
        assert_eq!(layer_of("engine"), None);
    }
}
