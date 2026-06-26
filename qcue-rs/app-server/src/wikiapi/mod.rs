//! QCue S3 — the wiki READ surface (index + page+backlinks). Named `wikiapi` (not `wiki`) so it does not
//! shadow the extern `wiki` crate the recall/ingest paths depend on. See `routes.rs`.
pub mod routes;
