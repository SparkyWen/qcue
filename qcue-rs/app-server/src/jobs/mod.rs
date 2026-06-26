//! QCue S3 — the durable job queue: SKIP-LOCKED claim + per-tenant bound (queue), the Tokio worker
//! pool + JobHandler seam + lease reclaim (worker), and the `GET /v1/jobs/{id}` poll (routes).
pub mod queue;
pub mod routes;
pub mod spawn;
pub mod worker;
