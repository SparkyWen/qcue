-- QCue Appendix B §4.15 `jobs` (lease state machine — ingest/lint/dream/transcribe; Clariose §2,§8).
-- Table/indexes/trigger copied VERBATIM from the appendix; the `-- RLS(jobs)` shorthand is expanded to
-- the §4 block (the trailing GRANT routed through `_grant_app()`, skipped when the app role is
-- unprovisioned). Claimed via `SELECT … FOR UPDATE SKIP LOCKED` by Tokio worker tasks (no BullMQ;
-- RKM §6). The `job_state`/`job_kind` enums are declared once in the §2.1 prelude (00000).

CREATE TABLE jobs (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id       UUID REFERENCES users(id) ON DELETE CASCADE,
  kind          job_kind NOT NULL,
  state         job_state NOT NULL DEFAULT 'pending',
  payload       JSONB NOT NULL DEFAULT '{}',          -- e.g. {idea_id} for ingest, {} for dream
  result        JSONB,                                -- IngestReport / lint summary / dream summary
  -- lease columns (FOR UPDATE SKIP LOCKED claim) ──
  lease_holder  TEXT,                                 -- worker id holding the lease
  lease_expires TIMESTAMPTZ,                          -- stale-reclaim boundary
  attempt_count INT NOT NULL DEFAULT 0,
  max_attempts  INT NOT NULL DEFAULT 5,
  last_error    TEXT,
  priority      INT NOT NULL DEFAULT 0,
  available_at  TIMESTAMPTZ NOT NULL DEFAULT now(),   -- debounce / backoff schedule
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- the claim scan: pending+available jobs for a tenant, highest priority/oldest first
CREATE INDEX jobs_claim_idx ON jobs (tenant_id, state, priority DESC, available_at)
  WHERE state = 'pending';
-- stale-lease reclaim scan
CREATE INDEX jobs_lease_idx ON jobs (tenant_id, state, lease_expires) WHERE state = 'leased';
CREATE TRIGGER jobs_touch BEFORE UPDATE ON jobs
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(jobs)
ALTER TABLE jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE jobs FORCE  ROW LEVEL SECURITY;
CREATE POLICY jobs_tenant_isolation ON jobs
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('jobs');
