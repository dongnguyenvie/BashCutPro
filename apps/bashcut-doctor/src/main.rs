//! `bashcut-doctor`: check this machine before a run. Exit 0 when nothing failed, 1 otherwise, 2 on bad usage.

use std::path::PathBuf;

use bashcut_doctor::{Options, RealEnv, run_checks, updates};

const HELP: &str = "\
bashcut-doctor: check bashcutpro (or filmcraft-cli), the kit, plugins, models, agent CLIs, disk, memory and GPU

USAGE
  bashcut-doctor [--json] [--updates] [--no-hash] [--cli <path>] [--data-dir <dir>] [--kit <dir>]
                 [--plugins <dir>] [--registry <path|url>] [--min-free-gb N] [--min-ram-gb N]

  --json        one JSON object (schema bashcut-doctor/1) for the batch runner and the Runs section
  --updates     also compare installed versions with the registry and the release list (reports only)
  --no-hash     check model file sizes only (hashing large models takes a while)
  --version     print `bashcut-doctor <version>` and exit
";

fn main() {
    let mut opts = Options::default();
    let (mut json, mut updates_too) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("bashcut-doctor: {name} needs a value\n\n{HELP}");
                std::process::exit(2)
            })
        };
        match a.as_str() {
            "--json" => json = true,
            "--updates" => updates_too = true,
            "--no-hash" => opts.hash_models = false,
            "--cli" => opts.cli = Some(value("--cli")),
            "--data-dir" => opts.data_dir = Some(PathBuf::from(value("--data-dir"))),
            "--kit" => opts.kit_dir = Some(PathBuf::from(value("--kit"))),
            "--plugins" => opts.plugins_dir = Some(PathBuf::from(value("--plugins"))),
            "--registry" => opts.registry = Some(value("--registry")),
            "--min-free-gb" => opts.min_free_gb = value("--min-free-gb").parse().unwrap_or(opts.min_free_gb),
            "--min-ram-gb" => opts.min_ram_gb = value("--min-ram-gb").parse().unwrap_or(opts.min_ram_gb),
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("bashcut-doctor {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("bashcut-doctor: unknown argument `{other}`\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    let env = RealEnv;
    let mut report = run_checks(&env, &opts);
    if updates_too {
        report.findings.extend(updates::check_updates(&env, &opts));
    }
    if json {
        println!("{}", report.to_json());
    } else {
        print!("{}", report.to_text());
    }
    std::process::exit(report.exit_code());
}
