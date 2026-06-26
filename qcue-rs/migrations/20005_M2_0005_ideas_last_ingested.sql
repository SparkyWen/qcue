-- QCue DIG-R1/DIG-R2 (spec 2026-06-16 §5) — incremental digest needs to know WHEN each capture was last
-- distilled into the wiki, so the dirty scan can pick up edited-since-ingest captures (not just 'pending').
-- `last_ingested_at` is NULL until the first successful ingest; `IngestJob::run` stamps it = now() on the
-- 'ingested' transition (alongside ingest_state='ingested'). The dirty predicate is:
--   active AND (ingest_state='pending' OR (last_ingested_at IS NOT NULL AND updated_at > last_ingested_at))
-- The 'pending' arm is already served by ideas_pending_idx (migration 20003); this partial index serves the
-- edited-since-ingest arm. Tenant-led (B-R3). The `updated_at` trigger (ideas_touch, 20003:35) already bumps
-- updated_at on every UPDATE, so an edited capture body lifts updated_at past last_ingested_at automatically.
ALTER TABLE ideas ADD COLUMN last_ingested_at TIMESTAMPTZ;

CREATE INDEX ideas_dirty_idx ON ideas (tenant_id, updated_at)
  WHERE active AND last_ingested_at IS NOT NULL;
