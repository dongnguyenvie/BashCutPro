/// (upstream text, ours) for the tool results of `filmcraft_automation`'s MCP server that name the
/// app (BR4); shared by `mcp_brand.rs` and `build.rs`.
pub const TEXTS: &[(&str, &str)] = &[
    ("screenshot of the live FilmCraft window", "screenshot of the live BashCutPro window"),
    (
        "this tool drives the live app: run `filmcraft-cli mcp --bridge 127.0.0.1:<port>` with the app started as `filmcraft --control <port>`",
        "this tool drives the live app: open BashCutPro and run `bashcutpro-cli mcp` (it connects to the open app), or `bashcutpro-cli mcp --bridge 127.0.0.1:<port>` with the app started with `--control <port>`",
    ),
];
