# xumlskg cui-embedding

Enriches UMLS concepts with a vector `embedding` property, computed from each concept's textual description and written back onto the node in a form a Neo4j vector index can consume.

## Overview

This subcommand connects to a live Neo4j instance holding a UMLS graph (as produced by [umls2kg](https://github.com/c2fc2f/UMLS-to-KG)), finds every `UMLSConcept` that has no `embedding` yet, and for each one:

1. **Resolves a description.** UMLS concepts do not all carry a definition, so the text is resolved through a hierarchical fallback (see [Text resolution](#text-resolution) below).
2. **Embeds it.** Every resolved string is sent to the configured provider (Ollama or an OpenAI-compatible API) and turned into a vector.
3. **Pools the vectors.** When a concept resolves to several strings, their vectors are averaged component-wise (mean pooling) into a single vector.
4. **Writes it back.** The pooled vector is stored on the node with `db.create.setNodeVectorProperty(c, 'embedding', …)`, the procedure Neo4j expects for vector-index-backed properties.

The scan is driven by a single Cypher query and processed concurrently; concepts that resolve to no text at all are logged and skipped. The run is **idempotent and resumable** — because it only ever looks at concepts where `embedding IS NULL`, interrupting it and starting again simply continues where it left off.

## Text resolution

A concept's embedding is only as good as the text it is built from. `UMLSConcept` nodes are not guaranteed to have their own definition, so the query falls back through progressively looser sources and uses the first non-empty one:

1. **The concept's own definitions** — `(:UMLSConcept)-[:HAS_DEFINITION]->(:UMLSDefinition)`.
2. **The preferred atom's definitions** — follow the preferred lexical chain `(:UMLSConcept)<-[:IS_LEXICAL_OF {isPreferred:true}]-(:UMLSLexical)<-[:IS_STRING_OF {isPreferred:true}]-(:UMLSString)<-[:IS_ATOM_OF {isPreferred:true}]-(:UMLSAtom)`, then take that atom's definitions.
3. **Any atom's definitions** — the same chain without the `isPreferred` constraint.
4. **The preferred atom's name** — the string value of the preferred atom itself, when no definition exists anywhere.
5. **Any atom's name** — the string value of any atom, as a last resort.

In short: real definitions are preferred over names, and preferred forms over arbitrary ones. The node and relationship labels above are exactly those emitted by [umls2kg](https://github.com/c2fc2f/UMLS-to-KG).

## Requirements

- A running Neo4j instance (Bolt protocol) loaded with a UMLS graph produced by [umls2kg](https://github.com/c2fc2f/UMLS-to-KG)
- An embedding provider:
  - an [Ollama](https://ollama.com/) instance with the chosen embedding model pulled, **or**
  - an OpenAI-compatible API endpoint and key

## Usage

```
xumlskg cui-embedding --provider <PROVIDER> --model <MODEL> [OPTIONS]
```

### Provider options

| Flag | Short | Description | Default |
|---|---|---|---|
| `--provider <PROVIDER>` | | Embedding API provider: `openai` or `ollama` | *(required)* |
| `--model <MODEL>` | `-m` | Embedding model identifier passed to the provider | *(required)* |
| `--base-url <URL>` | | Custom base URL for the provider's API endpoint | *(provider default)* |
| `--api-key <KEY>` | | Authentication key for the provider. Also read from `PROVIDER_API_KEY`. **Required** for `openai`; optional for `ollama` | *(none)* |
| `--parallel <N>` | `-p` | Number of concepts embedded concurrently | `1` |

### Neo4j options

| Flag | Short | Description | Default |
|---|---|---|---|
| `--uri <URI>` | | Bolt URI of the target instance. Accepted formats: `host:port`, `bolt://host:port`, or `neo4j://host:port`. Also read from `NEO4J_URI` | `127.0.0.1:7687` |
| `--user <USER>` | | Neo4j username. Also read from `NEO4J_USER` | `neo4j` |
| `--password <PASSWORD>` | | Neo4j password. Also read from `NEO4J_PASSWORD`, which avoids exposing it in shell history | `neo4j` |
| `--database <DATABASE>` | | Target database within the instance. Also read from `NEO4J_DB` | `neo4j` |

The connection pool is sized to `--parallel` + 1, so raising the concurrency raises the number of simultaneous Bolt connections accordingly.

## Examples

Embed every concept with a local Ollama model, eight at a time:

```bash
xumlskg cui-embedding \
  --provider ollama \
  --model embeddinggemma:latest \
  --parallel 8
```

Use the OpenAI API, passing the key through the environment and targeting a remote database:

```bash
PROVIDER_API_KEY=sk-… \
NEO4J_PASSWORD=secret \
xumlskg cui-embedding \
  --provider openai \
  --model text-embedding-3-small \
  --uri bolt://db.example.com:7687 \
  --database umls
```

Point Ollama at a non-default host with a custom base URL:

```bash
xumlskg cui-embedding \
  --provider ollama \
  --base-url http://gpu-box:11434 \
  --model nomic-embed-text \
  --parallel 4
```

## Output

The subcommand writes nothing to standard output; its effect is a new property on the graph. Every embedded `UMLSConcept` gains:

| Property | Type | Description |
|---|---|---|
| `embedding` | `LIST<FLOAT>` (vector) | Mean-pooled embedding of the concept's resolved description, written with `db.create.setNodeVectorProperty` |

To make these vectors searchable, create a vector index over the property, for example:

```cypher
CREATE VECTOR INDEX umls_concept_embedding IF NOT EXISTS
FOR (c:UMLSConcept) ON (c.embedding)
OPTIONS { indexConfig: {
  `vector.dimensions`: 768,
  `vector.similarity_function`: 'cosine'
} };
```

Set `vector.dimensions` to match the chosen model's output size. Once the index exists, the graph is ready for retrieval-augmented querying with [kag](https://github.com/c2fc2f/kag).

## Notes

- Run with `-v`/`-vv` to surface per-concept progress: how many definitions were resolved, the pooled vector's dimension, and any concepts skipped for lack of text.
- `--api-key` and `--password` are redacted from debug logs.
- All of a concept's texts are embedded by the same model, so they share a dimension. If a provider ever returns vectors of differing lengths, the run stops with a clear error instead of writing a corrupt vector.

See the [project README](../../../README.md) for installation, global options, and shell completions.
