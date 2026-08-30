//! The terminal frontend's entry point.

use parallex::core::cli::{Cli, parse_args};

fn main() {
    match parse_args(std::env::args().skip(1), "parallex-tui") {
        Cli::Exit(code) => std::process::exit(code),
        Cli::Launch(file) => {
            if let Err(e) = parallex::tui::run(file) {
                eprintln!("parallex-tui: {e}");
                std::process::exit(1);
            }
        }
    }
}
