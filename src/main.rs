//! A multitool for extending UMLS knowledge graphs (CSV-based for Neo4J) with
//! additional nodes, relationships, and external metadata

pub mod cli;
mod subcommand;

use cli::{Args, Command};

use std::io::stdout;

use clap::{CommandFactory, Parser};
use clap_complete::generate;
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  let filter = EnvFilter::builder()
    .with_default_directive(args.verbosity.tracing_level_filter().into())
    .from_env_lossy();

  fmt()
    .with_env_filter(filter)
    .with_writer(std::io::stderr)
    .init();

  tracing::debug!(version = env!("CARGO_PKG_VERSION"), "logging initialised");

  match args.command {
    Command::Completion(a) => {
      tracing::debug!(shell = ?a.shell, "writing shell completion to stdout");
      let mut cmd = cli::Args::command();
      let name = cmd.get_name().to_string();
      generate(a.shell, &mut cmd, name, &mut stdout());
      Ok(())
    }
    Command::CuiEmbedding(a) => subcommand::cui_embedding::run(a),
  }
}

