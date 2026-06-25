//! `cui-embedding` subcommand:

use core::fmt;
use std::num::NonZero;

use clap::ValueEnum;

/// Enriches UMLS Concepts with an additional "embedding" property
#[derive(clap::Args)]
pub struct Args {
  /// The API provider to use for generating the embeddings
  #[arg(long)]
  pub provider: Provider,

  /// An optional custom base URL for the provider's API endpoint
  #[arg(long)]
  pub base_url: Option<String>,

  /// The authentication key for the selected provider
  #[arg(long, env = "PROVIDER_API_KEY")]
  pub api_key: Option<String>,

  /// The specific model to use for the embeddings
  #[arg(short, long)]
  pub model: String,

  /// Number of parallel tasks
  #[arg(short, long, default_value = "1")]
  pub parallel: NonZero<usize>,

  /// Bolt URI of the target instance.
  ///
  /// Accepted formats: `host:port`, `bolt://host:port`, or
  /// `neo4j://host:port`. The driver normalises all three internally.
  #[arg(long, env = "NEO4J_URI", default_value = "127.0.0.1:7687")]
  pub uri: String,

  /// Neo4j username used for authentication.
  #[arg(long, env = "NEO4J_USER", default_value = "neo4j")]
  pub user: String,

  /// Neo4j password used for authentication.
  ///
  /// Can also be supplied via the `NEO4J_PASSWORD` environment variable
  /// to avoid exposing credentials in shell history.
  #[arg(long, env = "NEO4J_PASSWORD", default_value = "neo4j")]
  pub password: String,

  /// Name of the target database within the Neo4j instance.
  #[arg(long, env = "NEO4J_DB", default_value = "neo4j")]
  pub database: String,
}

impl fmt::Debug for Args {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    const REDACTED: &str = "[REDACTED]";

    f.debug_struct("Args")
      .field("provider", &self.provider)
      .field("base_url", &self.base_url)
      .field("api_key", &self.api_key.as_ref().map(|_| REDACTED))
      .field("model", &self.model)
      .field("parallel", &self.parallel)
      .field("uri", &self.uri)
      .field("user", &self.user)
      .field("password", &REDACTED)
      .field("database", &self.database)
      .finish()
  }
}

/// Supported API providers for generating text embeddings
#[derive(Clone, Debug, ValueEnum)]
pub enum Provider {
  /// OpenAI's remote embedding API
  #[value(name = "openai")]
  OpenAI,
  /// A Ollama instance
  Ollama,
}
