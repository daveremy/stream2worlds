//! The `s2w` command-line tool.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("s2w {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!(
                "s2w: point it at an event stream and a world model forms.\n\nNothing runs yet: the harness is being built (gate 2).\n\nUsage: s2w --version"
            );
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("s2w: unknown argument '{other}'. Try: s2w --help");
            ExitCode::from(2)
        }
    }
}
