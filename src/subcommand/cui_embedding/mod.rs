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

use crate::cli::cui_embedding::{Args, Provider};

/// A dynamic dispatcher for text embedding models from various LLM providers
pub enum DynamicEmbeder {
  /// Wraps an [`ollama::EmbeddingModel`].
  Ollama(ollama::EmbeddingModel),

  /// Wraps an [`openai::EmbeddingModel`].
  OpenAI(openai::EmbeddingModel),
}

impl DynamicEmbeder {
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
  pub async fn embed_texts(
    &self,
    texts: Vec<String>,
  ) -> Result<Vec<Embedding>, EmbeddingError> {
    match self {
      Self::Ollama(a) => a.embed_texts(texts).await,
      Self::OpenAI(a) => a.embed_texts(texts).await,
    }
  }
}

/// Entry point to this command
pub fn run(args: Args) -> anyhow::Result<()> {
  let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .context("Failed building the Runtime")?;

  let _ = rustls::crypto::ring::default_provider().install_default();

  let embeder = match args.provider {
    Provider::Ollama => {
      let mut client = ollama::Client::builder()
        .api_key(args.api_key.as_deref().unwrap_or_default());
      if let Some(base_url) = args.base_url {
        client = client.base_url(base_url);
      }
      client.build().map(|c| {
        Arc::new(DynamicEmbeder::Ollama(c.embedding_model(args.model)))
      })
    }
    Provider::OpenAI => {
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
        client = client.base_url(base_url);
      }
      client.build().map(|c| {
        Arc::new(DynamicEmbeder::OpenAI(c.embedding_model(args.model)))
      })
    }
  }
  .context("Error during initialization of the embedding model")?;

  let config = neo4rs::ConfigBuilder::default()
    .uri(&args.uri)
    .user(args.user)
    .password(args.password)
    .db(args.database.as_str())
    .max_connections(args.parallel.get())
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

  let query = query(
    "MATCH (n:UMLSConcept) WHERE n.embedding IS NULL RETURN elementId(n)",
  );

  rt.block_on(async {
    graph
      .execute(query)
      .await
      .context("Failed to retrieve UMLSConcept with no embedding property")?
      .into_stream()
      .map_err(|e| anyhow::anyhow!(e))
      .try_for_each_concurrent(args.parallel.get(), |row| {
        let embeder = Arc::clone(&embeder);
        let graph = Arc::clone(&graph);

        async move {
          let concept: &str = row.get("elementId(n)").context(
            "\
              Failed to retrieve UMLSConcept ({concept}) with no embedding \
              property\
            ",
          )?;

          add_embedding(concept, &graph, &embeder).await
        }
      })
      .await
  })
}

/// Resolves text definitions for a given UMLS Concept ID, computes their
/// embeddings, averages them via mean pooling, and updates the node vector
/// property in Neo4j.
async fn add_embedding(
  concept: &str,
  graph: &Graph,
  embeder: &DynamicEmbeder,
) -> anyhow::Result<()> {
  let q = query(
    "\
      MATCH (n:UMLSConcept)
      WHERE elementId(n) = $id

      OPTIONAL MATCH (n)-[:HAS_DEFINITION]->(d0:UMLSDefinition)
      WITH n, collect(d0.value) AS d0_list

      CALL (n, d0_list) {
        WITH * 
        WHERE size(d0_list) = 0 
        OPTIONAL MATCH 
          (n)<-[:IS_LEXICAL_OF { isPreferred: true }]-(:UMLSLexical)
             <-[:IS_STRING_OF  { isPreferred: true }]-(:UMLSString)
             <-[:IS_ATOM_OF    { isPreferred: true }]-(a0:UMLSAtom)
        OPTIONAL
          MATCH (a0)-[:HAS_DEFINITION]->(d1:UMLSDefinition)
        RETURN
          collect(DISTINCT d1.value) AS d1_list,
          collect(DISTINCT a0.value) AS a0_list
      }

      CALL (n, d0_list, d1_list) {
        WITH * 
        WHERE
          size(d0_list) = 0 AND
          size(d1_list) = 0 
        OPTIONAL MATCH
          (n)<-[:IS_LEXICAL_OF]-(:UMLSLexical)
             <-[:IS_STRING_OF ]-(:UMLSString)
             <-[:IS_ATOM_OF   ]-(a1:UMLSAtom)
        OPTIONAL MATCH
          (a1)-[:HAS_DEFINITION]->(d2:UMLSDefinition)
        RETURN
          collect(DISTINCT d2.value) AS d2_list,
          collect(DISTINCT a1.value) AS a1_list
      }

      RETURN CASE
        WHEN size(d0_list) > 0 THEN d0_list
        WHEN size(d1_list) > 0 THEN d1_list
        WHEN size(d2_list) > 0 THEN d2_list
        WHEN size(a0_list) > 0 THEN a0_list
        ELSE a1_list
      END AS definitions\
    ",
  )
  .param("id", concept);

  let defs: Vec<String> = graph
    .execute(q)
    .await
    .with_context(|| {
      format!(
        "\
          Failed to retrieve UMLSConcept ({concept}) with no embedding \
          property\
        "
      )
    })?
    .next()
    .await?
    .context("No record found in the query result set")?
    .get("definitions")
    .context("The 'definitions' property is missing or invalid")?;

  if defs.is_empty() {
    return Err(anyhow::anyhow!(
      "No defs were retrieved for the concept ({concept})"
    ));
  }

  let embeddings = embeder.embed_texts(defs).await.context(
    "Failed to generate text embeddings for the retrieved definitions",
  )?;

  let first = embeddings
    .first()
    .context("No embeddings were generated for the definitions")?;

  let dim = first.vec.len();
  let count = embeddings.len() as f64;

  let mut pooled_vector = vec![0.0_f64; dim];
  for embedding in &embeddings {
    for (acc, &value) in pooled_vector.iter_mut().zip(&embedding.vec) {
      *acc += value;
    }
  }
  for val in &mut pooled_vector {
    *val /= count;
  }

  let q = query(
    "\
      MATCH (c:UMLSConcept)
      WHERE elementId(c) = $id
      CALL db.create.setNodeVectorProperty(c, 'embedding', $embedding)\
   ",
  )
  .param("id", concept)
  .param("embedding", pooled_vector);

  graph
    .run(q)
    .await
    .context("Failed to write pooled embedding for concept")
}
