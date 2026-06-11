#![forbid(unsafe_code)]

pub mod classify;
pub mod clean;
pub mod cli;
pub mod counter;
pub mod detect;
pub mod render;
pub mod runner;
pub mod util;

pub use runner::{install_skills_into, run, ExtractReport, InstallReport, RunOutcome, StatsReport};

pub fn run_cli() -> i32 {
    use clap::Parser;

    let args = cli::Cli::parse();
    match runner::run(args) {
        Ok(outcome) => {
            outcome.print();
            0
        }
        Err(err) => {
            eprintln!("ERROR: {err:#}");
            1
        }
    }
}
