//! A stand-in for `filmcraft-cli` used by `tests/wrapper.rs`: prints its arguments (and stdin with
//! `--stdin`) on stdout, a line on stderr, and exits with `--exit N` (default 0).

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = String::new();
    if args.iter().any(|a| a == "--stdin") {
        let _ = std::io::stdin().read_to_string(&mut input);
    }
    let code = args.iter().position(|a| a == "--exit").and_then(|i| args.get(i + 1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let mut out = std::io::stdout();
    let _ = writeln!(out, "args {}", args.join("|"));
    let _ = write!(out, "stdin {input}");
    eprintln!("err from engine");
    std::process::exit(code);
}
