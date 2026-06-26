# Architecture (Public Overview)

A public-safe overview of how QCue is built. It intentionally omits production domains, deployment
topology, infrastructure configuration, credentials, and internal operational detail; those are not
part of the public repository. Where a concrete environment value would otherwise be required, it is
marked `TODO`.

## Three code areas

- **`qcue-rs/`** — a Rust workspace containing the LLM **harness**, the **idea engine**, and a
  multi-tenant **backend** (Axum).
- **`qcue_app/`** — a Flutter app (Android + iOS), offline-first, with a fully keyless/networkless
  stub mode.
- **`design-system/`** — shared design tokens and visual language.

## The harness

The harness is the load-bearing seam between QCue and any LLM provider. Its goal: **adding a provider
is data, not new branches in the main loop.**

- A single turn loop drives a conversation and never branches on provider name; it reaches a provider
  only through a small dispatch trait. One implementation is a scripted stub (used by tests and by the
  app's stub mode); the other performs live HTTP calls.
- A provider is described declaratively (a profile plus a few stateless hooks and wire-quirk table
  entries) rather than by bespoke code paths.
- Everything normalizes to one internal message shape and one internal stream-event shape, so a
  session can fall back from one provider to another without re-encoding. Raw vendor JSON does not
  escape the API layer.
- Credentials live in a pool with explicit health states; errors are classified once, and a retry
  loop maps the classification to exactly one of rotate / fall back / back off / abort.

## Crate layering law

The workspace is a strict acyclic stack — a crate may depend only on lower layers:

```
protocol → http → llm-api → providers → router → {wiki, ideas} → {backend, ffi}
```

- `protocol/` is serialization-only (no async, no I/O); anything crossing the Rust↔Dart boundary
  belongs there.
- A workspace lint task enforces the layering law and the serialization-only purity of `protocol/`
  in CI.

## Idea engine

- **Dual representation.** The Markdown body in a per-tenant vault is the source of truth; a database
  mirrors structure and frontmatter for fast queries and linting. A single write gate is the only
  place that writes wiki bodies: it sanitizes links, updates the mirror, and writes the file.
- **Recall** is agentic: the model drives a search tool over full-text search plus curated memory and
  passive prefetch.
- **Consolidation** ("Auto-Dream") runs as a cron-style background pass with cost checked **before**
  any provider call, and proposes edits through a reversible approve gate.

## Backend

- **Multi-tenancy via database row-level security**, not application-level filtering: each request
  binds a tenant context, and every query runs under that context.
- The surface includes authentication, an encrypted bring-your-own-key vault, a job queue,
  server-sent-event and websocket channels for live turns and recall, and a sync hub.
- Background workers and the consolidation cron are feature-gated and disabled by default.
- Deployment specifics (hosts, certificates, secrets, service management) are intentionally **not**
  included in the public repository. `TODO:` a generic, self-host-oriented deployment guide may be
  added later.

## Flutter app

- **One data seam.** A single API-client interface has a stub implementation (fixtures, keyless) and
  an HTTP implementation; an offline-aware decorator captures writes locally first and flushes
  idempotently, so screens are unaware of connectivity.
- **State management** uses a single reactive approach; each screen renders one sealed state variant
  (loading / empty / error / data).
- **Design tokens** are centralized; raw colors are confined to the token layer and enforced by an
  architecture test.
- All capture paths (text, voice, share, widget) funnel through one idempotent capture call.

## Rust ↔ Dart codegen

Wire types are defined once in Rust and exported to a shared schema and Dart models by a codegen
step; a drift test fails CI if the checked-in artifacts are stale.

## Running locally

See the [README](../README.md) quickstart. The app's stub mode runs the entire product with no API
key and no network, which is the fastest way to explore the architecture end to end.
