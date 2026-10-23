//! Windows only: the BashCutPro icon and version info in `bashcut-mcp.exe` (PK4, `bashcut-winres`).

fn main() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    bashcut_winres::embed("bashcut-mcp.exe", "BashCutPro MCP server for agents")
}
