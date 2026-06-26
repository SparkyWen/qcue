//! QCue S3 — the capture API: persist-before-enqueue, untrusted fencing/escaping, and the reverse-
//! chron feed. The ingest LOGIC is S2; this surface only persists the idea + queues a `kind='ingest'` job.
pub mod routes;
