-- QCue Appendix B §4.19 `approvals` (Approval Center — fail-closed; RKM §7 #5) + §4.20 `audit_log`
-- (mandatory audit; RKM §7 #5; Clariose `audit_logs`). Tables/indexes/trigger copied VERBATIM; the
-- `-- RLS(approvals)` / `-- RLS(audit_log)` shorthands are expanded to the §4 block (the trailing
-- GRANT routed through `_grant_app()`, skipped when the app role is unprovisioned). The
-- `approval_status` enum is declared once in the §2.1 prelude (00000). `audit_log.detail` passes
-- through the central redactor (B-R11) at the Rust persistence boundary — secrets/keys never land here.

-- ============================ §4.19 approvals ============================
CREATE TABLE approvals (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  action      TEXT NOT NULL,                           -- 'wiki_merge'|'wiki_delete'|'schema_apply'|'external_send'|'paid_action'
  subject_ref JSONB NOT NULL DEFAULT '{}',             -- what is being approved (page ids, etc.)
  status      approval_status NOT NULL DEFAULT 'pending',
  requested_by TEXT NOT NULL,                          -- 'dream'|'ingest'|'lint'|'user'
  decided_by  UUID REFERENCES users(id) ON DELETE SET NULL,
  decided_at  TIMESTAMPTZ,
  expires_at  TIMESTAMPTZ,                             -- pending → expired (fail-closed default)
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX approvals_pending_idx ON approvals (tenant_id, status, created_at) WHERE status = 'pending';
CREATE TRIGGER approvals_touch BEFORE UPDATE ON approvals
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(approvals)
ALTER TABLE approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE approvals FORCE  ROW LEVEL SECURITY;
CREATE POLICY approvals_tenant_isolation ON approvals
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('approvals');

-- ============================ §4.20 audit_log ============================
CREATE TABLE audit_log (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
  action      TEXT NOT NULL,                           -- 'auth.login.ok'|'auth.login.failed'|'cred.add'|'dream.run'|'approval.decide'|…
  resource    TEXT,
  resource_id UUID,
  detail      JSONB NOT NULL DEFAULT '{}',             -- redacted at the persistence boundary (B-R11)
  ip_hash     TEXT,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_tenant_idx ON audit_log (tenant_id, created_at DESC);
CREATE INDEX audit_log_action_idx ON audit_log (tenant_id, action, created_at DESC);
-- RLS(audit_log)
ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log FORCE  ROW LEVEL SECURITY;
CREATE POLICY audit_log_tenant_isolation ON audit_log
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('audit_log');
