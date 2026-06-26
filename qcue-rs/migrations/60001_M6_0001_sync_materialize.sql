-- QCue M6_0001 — make the M5 sync_ops op-log materialize into the canonical tables.
-- Adds a monotonic, gap-free pull cursor (seq) and the wiki conflict/version columns.
-- Spec: docs/superpowers/specs/2026-06-15-multiplatform-sync-design.md (SYNC-D4/D2/D6).

-- SYNC-D4: a server-assigned, gap-free cursor for pull-since (the old hlc_wall_ms cursor was lossy —
-- two ops sharing a wall-ms could be missed or re-sent). seq is insertion-ordered + monotonic per row.
ALTER TABLE sync_ops ADD COLUMN seq BIGINT GENERATED ALWAYS AS IDENTITY;
CREATE INDEX sync_ops_seq_idx ON sync_ops (tenant_id, seq);

-- SYNC-D6: per-page content hash so a warm client skips re-downloading unchanged bodies on snapshot.
-- SYNC-D2: a monotonic version bumped on each materialized body write — the base_version precondition
-- a later device edit checks against to detect a conflicting concurrent write.
ALTER TABLE wiki_pages ADD COLUMN content_hash TEXT;
ALTER TABLE wiki_pages ADD COLUMN sync_version BIGINT NOT NULL DEFAULT 0;
