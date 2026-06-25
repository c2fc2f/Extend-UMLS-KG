//! This module provides a CLI-driven utility to populate vector embeddings
//! for Neo4j `UMLSConcept` nodes
//!
//! It queries the graph database for concepts missing an embedding, falls
//! back through a hierarchical resolution process to extract relevant textual
//! descriptions (definitions or atoms), generates vector
//! representations using a dynamic provider (Ollama or OpenAI), and saves the
//! mean-pooled vector back into Neo4j using vector node properties

use std::sync::Arc;

use anyhow::Context;
use futures::TryStreamExt;
use neo4rs::{Graph, query};
use rig_core::{
  client::EmbeddingsClient,
  embeddings::{Embedding, EmbeddingError, EmbeddingModel},
  providers::{ollama, openai},
};
use tracing::{debug, info, instrument, trace, warn};

use crate::cli::cui_embedding::{Args, Provider};

/// A dynamic dispatcher for text embedding models from various LLM providers
pub enum DynamicEmbedder {
  /// Wraps an [`ollama::EmbeddingModel`].
  Ollama(ollama::EmbeddingModel),

  /// Wraps an [`openai::EmbeddingModel`].
  OpenAI(openai::EmbeddingModel),
}

impl DynamicEmbedder {
  /// Human-readable name of the active provider, used for log fields.
  fn provider_name(&self) -> &'static str {
    match self {
      Self::Ollama(_) => "ollama",
      Self::OpenAI(_) => "openai",
    }
  }

  /// Generates an embedding for the provided texts by delegating the request
  /// to the currently active LLM provider.
  ///
  /// # Arguments
  ///
  /// * `texts` - A list of string slice containing the content to be
  ///   embedded.
  ///
  /// # Errors
  ///
  /// Returns an [`EmbeddingError`] if the underlying provider encounters an
  /// issue. This can include network timeouts, authentication failures
  /// (e.g., an invalid API key), or API rate limits.
  #[instrument(
    skip_all,
    fields(provider = self.provider_name(), texts = texts.len())
  )]
  pub async fn embed_texts(
    &self,
    texts: Vec<String>,
  ) -> Result<Vec<Embedding>, EmbeddingError> {
    debug!("requesting embeddings from provider");

    let result = match self {
      Self::Ollama(a) => a.embed_texts(texts).await,
      Self::OpenAI(a) => a.embed_texts(texts).await,
    };

    match &result {
      Ok(embeddings) => {
        debug!(count = embeddings.len(), "provider returned embeddings")
      }
      Err(error) => warn!(%error, "provider failed to return embeddings"),
    }

    result
  }
}

/// Computes the component-wise mean of a set of equal-length vectors.
///
/// # Errors
///
/// Returns an error if `vectors` is empty, if any vector has dimension zero,
/// or if the vectors do not all share the same dimensionality.
fn mean_pool(vectors: &[&[f64]]) -> anyhow::Result<Vec<f64>> {
  let (first, rest) = vectors
    .split_first()
    .context("cannot mean-pool an empty set of vectors")?;

  let dim = first.len();
  anyhow::ensure!(dim > 0, "embedding vectors have dimension zero");

  let mut pooled = first.to_vec();
  for (index, vector) in rest.iter().enumerate() {
    anyhow::ensure!(
      vector.len() == dim,
      "vector {} has dimension {} but expected {dim}",
      index + 1,
      vector.len()
    );
    for (acc, &value) in pooled.iter_mut().zip(vector.iter()) {
      *acc += value;
    }
  }

  let count = vectors.len() as f64;
  for value in &mut pooled {
    *value /= count;
  }

  Ok(pooled)
}

/// Entry point to this command
#[instrument(
  skip_all,
  fields(
    provider = ?args.provider,
    model = %args.model,
    parallel = args.parallel.get(),
    uri = %args.uri,
    database = %args.database
  )
)]
pub fn run(args: Args) -> anyhow::Result<()> {
  info!("starting cui-embedding");

  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .context("Failed building the Runtime")?;

  let _ = rustls::crypto::ring::default_provider().install_default();

  let embedder = match args.provider {
    Provider::Ollama => {
      debug!("initialising ollama embedding client");
      let mut client = ollama::Client::builder()
        .api_key(args.api_key.as_deref().unwrap_or_default());
      if let Some(base_url) = args.base_url {
        debug!(%base_url, "using custom base url");
        client = client.base_url(base_url);
      }
      client.build().map(|c| {
        Arc::new(DynamicEmbedder::Ollama(c.embedding_model(args.model)))
      })
    }
    Provider::OpenAI => {
      debug!("initialising openai embedding client");
      let Some(api_key) = args.api_key else {
        return Err(anyhow::anyhow!(
          "\
            Error: Missing API key. An API key is strictly required when \
            using the OpenAI provider.\
          "
        ));
      };
      let mut client = openai::Client::builder().api_key(api_key);
      if let Some(base_url) = args.base_url {
        debug!(%base_url, "using custom base url");
        client = client.base_url(base_url);
      }
      client.build().map(|c| {
        Arc::new(DynamicEmbedder::OpenAI(c.embedding_model(args.model)))
      })
    }
  }
  .context("Error during initialization of the embedding model")?;

  debug!("embedding client initialised");

  debug!(
    uri = %args.uri,
    user = %args.user,
    database = %args.database,
    "connecting to neo4j"
  );

  let config = neo4rs::ConfigBuilder::default()
    .uri(&args.uri)
    .user(args.user)
    .password(args.password)
    .db(args.database.as_str())
    .max_connections(args.parallel.get() + 1)
    .build()
    .context(
      "Neo4j ConfigBuilder failed despite all credentials being supplied",
    )?;

  let graph = Arc::new(
    rt.block_on(neo4rs::Graph::connect(config))
      .with_context(|| {
        format!(
          "Failed to establish a connection to Neo4j database '{}' at {}",
          args.database, args.uri
        )
      })?,
  );

  info!("connected to neo4j");

  let query = query(
    "\
      MATCH (n:UMLSConcept)
      WHERE n.embedding IS NULL

      OPTIONAL MATCH (n)-[:HAS_DEFINITION]->(d0:UMLSDefinition)
      WITH n, collect(d0.value) AS d0_list

      CALL (n, d0_list) {
        WITH *
        WHERE size(d0_list) = 0
        OPTIONAL MATCH
          (n)<-[:IS_LEXICAL_OF { isPreferred: true }]-(:UMLSLexical)
             <-[:IS_STRING_OF  { isPreferred: true }]-(:UMLSString)
             <-[:IS_ATOM_OF    { isPreferred: true }]-(a0:UMLSAtom)
        OPTIONAL MATCH (a0)-[:HAS_DEFINITION]->(d1:UMLSDefinition)
        RETURN
          collect(DISTINCT d1.value) AS d1_list,
          collect(DISTINCT a0.value) AS a0_list
      }

      CALL (n, d0_list, d1_list) {
        WITH *
        WHERE size(d0_list) = 0 AND size(d1_list) = 0
        OPTIONAL MATCH
          (n)<-[:IS_LEXICAL_OF]-(:UMLSLexical)
             <-[:IS_STRING_OF ]-(:UMLSString)
             <-[:IS_ATOM_OF   ]-(a1:UMLSAtom)
        OPTIONAL MATCH (a1)-[:HAS_DEFINITION]->(d2:UMLSDefinition)
        RETURN
          collect(DISTINCT d2.value) AS d2_list,
          collect(DISTINCT a1.value) AS a1_list
      }

      RETURN
        elementId(n) AS id,
        CASE
          WHEN size(d0_list) > 0 THEN d0_list
          WHEN size(d1_list) > 0 THEN d1_list
          WHEN size(d2_list) > 0 THEN d2_list
          WHEN size(a0_list) > 0 THEN a0_list
          ELSE a1_list
        END AS definitions\
    ",
  );

  debug!("scanning for concepts missing an embedding");

  rt.block_on(async {
    graph
      .execute(query)
      .await
      .context("Failed to retrieve UMLSConcept with no embedding property")?
      .into_stream()
      .map_err(|e| anyhow::anyhow!(e))
      .try_for_each_concurrent(args.parallel.get(), |row| {
        let embedder = Arc::clone(&embedder);
        let graph = Arc::clone(&graph);

        async move {
          let id: &str =
            row.get("id").context("scan row missing 'id' column")?;
          let definitions: Vec<String> = row
            .get("definitions")
            .context("scan row missing 'definitions' column")?;

          add_embedding(id, definitions, &graph, &embedder).await
        }
      })
      .await
  })?;

  info!("finished cui-embedding");
  Ok(())
}

/// Computes the embedding for one concept from its pre-resolved definition
/// texts and writes the mean-pooled vector back onto the node.
///
/// # Errors
///
/// Returns an error if the provider fails, if the returned vectors cannot be
/// pooled, or if the write to Neo4j fails or times out.
#[instrument(
  skip_all,
  fields(concept = %concept, definitions = definitions.len())
)]
async fn add_embedding(
  concept: &str,
  definitions: Vec<String>,
  graph: &Graph,
  embedder: &DynamicEmbedder,
) -> anyhow::Result<()> {
  if definitions.is_empty() {
    warn!("no text resolved; skipping concept");
    return Ok(());
  }

  let wanted = definitions.len();
  debug!(definitions = wanted, "resolved definitions");

  let embeddings = embedder.embed_texts(definitions).await.context(
    "Failed to generate text embeddings for the retrieved definitions",
  )?;

  if embeddings.len() != wanted {
    warn!(
      wanted,
      got = embeddings.len(),
      "provider returned a different number of embeddings than inputs"
    );
  }

  let vectors: Vec<&[f64]> =
    embeddings.iter().map(|e| e.vec.as_slice()).collect();
  let pooled =
    mean_pool(&vectors).context("Failed to mean-pool embedding vectors")?;
  let dim = pooled.len();

  trace!(
    dim,
    vectors = embeddings.len(),
    "mean-pooled embedding vectors"
  );

  let q = query(
    "\
      MATCH (c:UMLSConcept)
      WHERE elementId(c) = $id
      CALL db.create.setNodeVectorProperty(c, 'embedding', $embedding)\
   ",
  )
  .param("id", concept)
  .param("embedding", pooled);

  graph
    .run(q)
    .await
    .context("Failed to write pooled embedding for concept")?;

  info!(definitions = embeddings.len(), dim, "stored embedding");
  Ok(())
}

/// Unit tests for the pure pooling logic.
#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
  use super::mean_pool;

  #[test]
  fn single_vector_is_returned_unchanged() {
    let v = [1.0, 2.0, 3.0];
    let pooled = mean_pool(&[v.as_slice()]).expect("pooling should succeed");
    assert_eq!(pooled, vec![1.0, 2.0, 3.0]);
  }

  #[test]
  fn averages_vectors_component_wise() {
    let a = [0.0, 2.0, 4.0];
    let b = [2.0, 4.0, 6.0];
    let pooled =
      mean_pool(&[a.as_slice(), b.as_slice()]).expect("pooling should succeed");
    assert_eq!(pooled, vec![1.0, 3.0, 5.0]);
  }

  #[test]
  fn empty_input_is_an_error() {
    assert!(mean_pool(&[]).is_err());
  }

  #[test]
  fn zero_dimension_is_an_error() {
    let empty: [f64; 0] = [];
    assert!(mean_pool(&[empty.as_slice()]).is_err());
  }

  #[test]
  fn dimension_mismatch_is_an_error() {
    let a = [1.0, 2.0];
    let b = [1.0, 2.0, 3.0];
    assert!(mean_pool(&[a.as_slice(), b.as_slice()]).is_err());
  }
}
