#![forbid(unsafe_code)]

pub mod classify;
pub mod clean;
pub mod cli;
pub mod counter;
pub mod detect;
pub mod query;
pub mod render;
pub mod runner;
pub mod util;

pub use runner::{install_skills_into, run, ExtractReport, InstallReport, RunOutcome, StatsReport};

pub fn run_cli() -> i32 {
    use clap::Parser;

    let args = cli::Cli::parse();
    match &args.command {
        Some(cli::Command::Sessions(sessions)) => return query::runner::run_sessions(sessions),
        Some(cli::Command::View(view)) => return query::runner::run_view(view),
        Some(cli::Command::Search(search)) => return query::runner::run_search(search),
        Some(cli::Command::Version(version)) => return query::runner::run_version(version),
        _ => {}
    }
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
