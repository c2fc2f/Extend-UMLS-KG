//! A multitool for extending UMLS knowledge graphs (CSV-based for Neo4J) with
//! additional nodes, relationships, and external metadata

pub mod cli;

use cli::{Args, Command};

use std::{io::stdout, process::ExitCode};

use clap::{CommandFactory, Parser};
use clap_complete::generate;

fn main() -> ExitCode {
  let args = Args::parse();

  match args.command {
    Command::Completion(a) => {
      let mut cmd = cli::Args::command();
      let name = cmd.get_name().to_string();
      generate(a.shell, &mut cmd, name, &mut stdout());
      ExitCode::SUCCESS
    }
  }
}
