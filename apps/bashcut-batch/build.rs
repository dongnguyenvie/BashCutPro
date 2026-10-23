//! Windows only: the BashCutPro icon and version info in `bashcut-batch.exe` (PK4, `bashcut-winres`).

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    bashcut_winres::embed("bashcut-batch.exe", "BashCutPro batch runner")
}
