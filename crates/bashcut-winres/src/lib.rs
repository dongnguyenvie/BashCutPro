//! Windows only: the BashCutPro icon and version info (VERSIONINFO) for our command-line tools
//! (`bashcutpro-cli`, `bcpro`, `bashcut-doctor`, `bashcut-batch`, `bashcut-mcp`; PK4), the way
//! `apps/filmcraft/build.rs` does it for the app, so Explorer, the file properties and the
//! installer show BashCutPro with the package version instead of a bare exe.
//!
//! A build-dependency only: each app's `build.rs` calls [`embed`]. On every other target it does
//! nothing. A missing resource compiler is a warning, unless `FILMCRAFT_REQUIRE_WINRES=1` (set by
//! the Windows packaging for release builds) turns it into a build error.

/// The brand icon, shared with the app (`bashcut/brand/app-icon/bashcutpro.ico`).
const ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bashcut/brand/app-icon/bashcutpro.ico");

/// Embeds the icon and VERSIONINFO (ProductName BashCutPro, `description` as FileDescription,
/// `file` as OriginalFilename, the package version) into the binaries of the calling package.
/// Call it from `build.rs`; it prints its own `rerun-if-*` lines.
pub fn embed(file: &str, description: &str) -> Result<(), String> {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-env-changed=FILMCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    match compile(file, description) {
        Ok(()) => Ok(()),
        Err(e) if std::env::var_os("FILMCRAFT_REQUIRE_WINRES").is_some() => Err(format!("embedding Windows resources into {file} failed: {e}")),
        Err(e) => {
            println!("cargo:warning={file} built without icon/version resources: {e}");
            Ok(())
        }
    }
}

#[cfg(windows)]
fn compile(file: &str, description: &str) -> Result<(), String> {
    let internal = file.strip_suffix(".exe").unwrap_or(file);
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON)
        .set("ProductName", "BashCutPro")
        .set("FileDescription", description)
        .set("CompanyName", "Birch Apps")
        .set("LegalCopyright", "Copyright (c) the BashCutPro and FilmCraft contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", file)
        .set("InternalName", internal);
    res.compile().map_err(|e| e.to_string())
}

/// Cross-compiling for Windows from macOS or Linux: `winresource` is not available on this host
/// (a warning even with `FILMCRAFT_REQUIRE_WINRES`, like the app).
#[cfg(not(windows))]
fn compile(file: &str, _description: &str) -> Result<(), String> {
    println!("cargo:warning={file} built without icon/version resources (cross-compiled from a non-Windows host)");
    Ok(())
}
