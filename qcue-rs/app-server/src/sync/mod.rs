//! QCue S3-R47..R50 — the CRDT sync hub. `routes` is the device-register, the idempotent ordered
//! op-push, and the HTTP surface (`/v1/sync/register`, `/v1/sync/push`, `/v1/sync/pull`); `materialize`
//! applies the unapplied `sync_ops` in total HLC order into the rebuild-safe materialized projection and
//! serves the pull-since cursor. The op-log is conflict-free: the unique `(tenant,device,site,lamport)`
//! makes a re-sent op an idempotent no-op, and the `(wall_ms, lamport, site_id)` total order means two
//! devices materialize identical state (B-R21 / D6).
pub mod emit;
pub mod materialize;
pub mod routes;
