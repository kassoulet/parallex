//! The gpui frontend's entry point. Parses the command line, then hands off to
//! the library.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use parallex::core::cli::{Cli, parse_args};
use parallex::gui::run;

fn main() {
    match parse_args(std::env::args().skip(1), "parallex-gpui") {
        Cli::Exit(code) => std::process::exit(code),
        Cli::Launch(file) => run(file),
    }
}
