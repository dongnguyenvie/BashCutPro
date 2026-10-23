//! BashCutPro (BR1): the app's own command-line output (`--version`, `--help`, usage errors) names the installed
//! `bashcutpro`, not upstream's `filmcraft`. Reached from `main.rs` by two inserted lines: `mod bashcut_cli;` and
//! [`tell_first!`] at the top of upstream's `tell` (`UPSTREAM.md` "Hooks"). Upstream's `args::USAGE` stays as it is;
//! [`branded`] rewrites the program name when the text is shown. Environment variable names (`FILMCRAFT_*`) are
//! real and stay.

/// The installed app's command name (`bashcutpro.exe`, `/usr/bin/bashcutpro`; `bashcut/rebrand.md`).
pub const BIN_NAME: &str = "bashcutpro";

/// `text` with upstream's program name replaced where it names the program: `filmcraft 0.10.0`, `filmcraft: <error>`
/// and `Usage: filmcraft …`.
pub fn branded(text: &str) -> String {
    let text = text.replace("Usage: filmcraft ", &format!("Usage: {BIN_NAME} "));
    match text.strip_prefix("filmcraft") {
        Some(rest) if rest.starts_with(' ') || rest.starts_with(':') => format!("{BIN_NAME}{rest}"),
        _ => text,
    }
}

/// Upstream's `tell` with our name: print `text` (stdout, or stderr for an error); a Windows release build has no
/// console, so there it is also shown in a message box titled BashCutPro. Always `true` (handled).
pub fn tell(text: &str, error: bool) -> bool {
    let text = branded(text);
    if error {
        eprint!("{text}");
    } else {
        print!("{text}");
    }
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    {
        let level = if error { rfd::MessageLevel::Error } else { rfd::MessageLevel::Info };
        let _ = rfd::MessageDialog::new().set_level(level).set_title(filmcraft_ui_egui::bashcut_brand::NAME).set_description(&text).show();
    }
    true
}

/// The hook at the top of upstream's `tell`: [`tell`] shows the text and upstream's body is skipped. A macro so the
/// hook stays one line.
macro_rules! tell_first {
    ($text:expr, $error:expr) => {
        if crate::bashcut_cli::tell($text, $error) {
            return;
        }
    };
}
pub(crate) use tell_first;

#[cfg(test)]
mod tests {
    use super::branded;

    #[test]
    fn version_usage_and_errors_name_bashcutpro() {
        assert_eq!(branded("filmcraft 0.10.0\n"), "bashcutpro 0.10.0\n");
        assert_eq!(branded("filmcraft: unknown option --x\n\nUsage: filmcraft [options]\n"), "bashcutpro: unknown option --x\n\nUsage: bashcutpro [options]\n");
        let usage = branded(crate::args::USAGE);
        assert!(usage.starts_with("Usage: bashcutpro [options]"), "{usage}");
        // the environment variables are real names and stay
        assert!(usage.contains("FILMCRAFT_CONTROL_PORT"));
    }

    #[test]
    fn other_text_is_left_alone() {
        assert_eq!(branded("filmcraftish\n"), "filmcraftish\n");
        assert_eq!(branded("no data dir: set FILMCRAFT_DATA_DIR\n"), "no data dir: set FILMCRAFT_DATA_DIR\n");
    }
}
