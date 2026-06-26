-- QCue Appendix B §4.1 `tenants` (global root — no tenant_id, no RLS). Copied VERBATIM; the only
-- adaptation is wrapping the final `GRANT SELECT ON tenants TO qcue_app` so it is skipped when the
-- app role is not provisioned (sqlx test fixture); the table/trigger DDL is byte-for-byte.

CREATE TABLE tenants (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  slug        TEXT NOT NULL UNIQUE,                  -- global (B-R6 exception)
  display_name TEXT NOT NULL,
  plan        TEXT NOT NULL DEFAULT 'solo',          -- D8: solo accounts only at v1
  status      TEXT NOT NULL DEFAULT 'active',        -- active|suspended
  -- per-tenant cost ceiling (micros/day); enforced by the controller before any provider call (D17)
  daily_cost_cap_micros BIGINT NOT NULL DEFAULT 5000000,   -- $5/day default (clariose RECALL_DAILY_CAP_USD)
  -- per-tenant Auto-Dream cadence overrides (D15 defaults: 24h / 5 sessions)
  dream_min_hours     INT NOT NULL DEFAULT 24,
  dream_min_sessions  INT NOT NULL DEFAULT 5,
  dream_enabled       BOOLEAN NOT NULL DEFAULT true,
  -- object-store / vault namespace key (§9). plain path chosen over hash (see §9.1 justification).
  namespace   TEXT NOT NULL,                         -- e.g. 't/<id>' ; immutable after create
  wiki_language TEXT NOT NULL DEFAULT 'en',          -- threaded into ingest prompts (wiki-engine §2 pitfall)
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER tenants_touch BEFORE UPDATE ON tenants
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- tenants is the root; readable only by the migrator + a narrow admin path. No RLS (it IS the key).
-- The app role may SELECT its own row by id (the auth extractor loads it once per session).
-- Appendix B: GRANT SELECT ON tenants TO qcue_app;  (wrapped for unprovisioned-role environments)
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'qcue_app') THEN
    GRANT SELECT ON tenants TO qcue_app;
  END IF;
END $$;
