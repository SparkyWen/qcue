-- QCue Appendix B §4.14 `session_kv` (per-session blackboard — versioned KV; RKM §5). Table +
-- UNIQUE (tenant_id, session_id, key) + trigger copied VERBATIM; the `-- RLS(session_kv)` shorthand
-- is expanded to the §4 block (GRANT via `_grant_app()`).

CREATE TABLE session_kv (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  session_id  UUID NOT NULL,
  key         TEXT NOT NULL,
  value       JSONB NOT NULL,
  version     INT NOT NULL DEFAULT 1,                 -- bumped in-tx on upsert (broadcast event on write)
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, session_id, key)
);
CREATE TRIGGER session_kv_touch BEFORE UPDATE ON session_kv
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(session_kv)
ALTER TABLE session_kv ENABLE ROW LEVEL SECURITY;
ALTER TABLE session_kv FORCE  ROW LEVEL SECURITY;
CREATE POLICY session_kv_tenant_isolation ON session_kv
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('session_kv');
