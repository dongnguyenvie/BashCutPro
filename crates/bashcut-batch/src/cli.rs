//! Which engine binary to run: the lookup lives in [`bashcut_dirs::engine`], shared with the other
//! consumers (RV2); re-exported here for the UI and `apps/bashcut-batch`.

pub use bashcut_dirs::engine::{ENV, NAMES, beside_self, default_cli, in_dir, on_path};
