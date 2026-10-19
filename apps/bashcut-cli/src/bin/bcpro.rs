//! `bcpro`: the short name of `bashcutpro` (the same program).

fn main() {
    std::process::exit(bashcut_cli::run(std::env::args_os().skip(1).collect()));
}
