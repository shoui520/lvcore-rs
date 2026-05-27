use std::path::PathBuf;

use clap::{Parser, Subcommand};
use lvcore::{DriverRegistry, Result};

#[derive(Debug, Parser)]
#[command(name = "lvcore")]
#[command(about = "Developer CLI for the lvcore reader library")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect package families and print stable metadata as JSON.
    Detect {
        /// Package root or payload path to inspect.
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Detect { path } => {
            let registry = DriverRegistry::default();
            let detected = registry.detect(&path)?;
            println!("{}", serde_json::to_string_pretty(&detected)?);
        }
    }
    Ok(())
}
