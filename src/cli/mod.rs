//! Command-line interface definition for the binary

pub mod completion;
pub mod cui_embedding;

use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;

/// A multitool for extending UMLS knowledge graphs (CSV-based for Neo4J) with
/// additional nodes, relationships, and external metadata
#[derive(Parser, Debug)]
#[command(version, about, long_about)]
pub struct Args {
  /// The specific operation to perform with the binary
  #[command(subcommand)]
  pub command: Command,

  /// Control the output verbosity (-v, -q)
  #[command(flatten)]
  pub verbosity: Verbosity,
}

/// List of available subcommands in the binary
#[derive(Subcommand, Debug)]
#[non_exhaustive]
pub enum Command {
  /// Enriches UMLS Concepts with an additional "embedding" property
  CuiEmbedding(cui_embedding::Args),
  /// Print shell completions and exit
  #[command(hide = true)]
  Completion(completion::Args),
}
