# xumlskg — Extend UMLS Knowledge Graph

A command-line multitool written in Rust for enriching UMLS knowledge graphs (CSV-based, targeting Neo4j) with additional nodes, relationships, and external metadata.

It is designed to extend a UMLS knowledge graph built with [umls2kg](https://github.com/c2fc2f/UMLS-to-KG), adding nodes, relationships, or properties drawn from external sources — either by editing the generated CSV files or by writing to a live [Neo4j](https://neo4j.com/) instance, depending on the subcommand.

## Overview

[umls2kg](https://github.com/c2fc2f/UMLS-to-KG) *produces* the UMLS graph as CSV files for bulk import into Neo4j. `xumlskg` adds extra information to that graph, sourced elsewhere. An extension is applied either by editing the generated CSV files before they are imported, or by writing to a live Neo4j instance once the graph is loaded — whichever fits the subcommand.

The first subcommand, `cui-embedding`, adds an `embedding` property to UMLS concepts. The resulting vectors are suitable for a Neo4j vector index and, in turn, for retrieval-augmented querying with [kag](https://github.com/c2fc2f/kag).

The project is a single Cargo package (no workspace). The `xumlskg` binary is the sole deliverable, built on:

- **`rig-core`** — provider abstraction for embedding models (Ollama, OpenAI)
- **`neo4rs`** — async Bolt client for Neo4j
- **`tokio`** — asynchronous runtime driving concurrent provider and database calls

## Requirements

- Rust toolchain (edition 2024, stable)
- A UMLS knowledge graph produced by [umls2kg](https://github.com/c2fc2f/UMLS-to-KG) — either its generated CSV files, or a running Neo4j instance already loaded with them, depending on the subcommand
- For subcommands that compute embeddings (`cui-embedding`): at least one embedding provider — a reachable [Ollama](https://ollama.com/) instance and/or an OpenAI-compatible API endpoint

## Installation

### From source

```bash
git clone https://github.com/c2fc2f/Extend-UMLS-KG
cd Extend-UMLS-KG
cargo build --release
# or
cargo install --git https://github.com/c2fc2f/Extend-UMLS-KG
```

The compiled binary will be at `target/release/xumlskg`.

### With Nix

A Nix flake is provided:

```bash
nix run github:c2fc2f/Extend-UMLS-KG -- --help
# or
nix build
# or, to enter a development shell:
nix develop
```

The Nix build also installs shell completions (bash, fish, zsh) and man pages.

## Usage

```
xumlskg [OPTIONS] <COMMAND>
```

Run `xumlskg --help` for the full list of subcommands, or `xumlskg <COMMAND> --help` for subcommand-specific options.

### Global options

| Flag | Short | Description | Default |
|---|---|---|---|
| `--verbose` | `-v` | Increase output verbosity (repeatable) | *(errors)* |
| `--quiet` | `-q` | Decrease output verbosity (repeatable) | |

Logs are written to standard error, so they never interfere with anything a subcommand prints to standard output (such as a generated completion script).

## Subcommands

| Subcommand | Description | Documentation |
|---|---|---|
| `cui-embedding` | Adds a vector `embedding` property to UMLS concepts | [README](src/subcommand/cui_embedding/README.md) |

A hidden `completion <SHELL>` subcommand prints a shell completion script to standard output.

## Shell completions

Generate a completion script for your shell and source it (the Nix package installs these automatically):

```bash
xumlskg completion bash > xumlskg.bash
xumlskg completion fish > xumlskg.fish
xumlskg completion zsh  > _xumlskg
```

## License

This project is licensed under the [MIT License](LICENSE).
