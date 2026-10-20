//! `bashcutpro`: see the crate docs (`src/lib.rs`).

fn main() {
    std::process::exit(bashcut_cli::run(std::env::args_os().skip(1).collect()));
}
