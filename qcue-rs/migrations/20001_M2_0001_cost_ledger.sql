-- QCue Appendix B §4.18 `cost_ledger` (per-tenant + per-user/day spend in micros — D17).
-- Table/index/trigger copied VERBATIM from the appendix (all 5 token columns INCLUDING
-- reasoning_tokens — the 5th CanonicalUsage field, a real ledger column summed by the D17 cost cap,
-- not telemetry-only). The `-- RLS(cost_ledger)` shorthand is expanded to the §4 block, with the
-- trailing GRANT routed through `_grant_app()` (skipped when the app role is unprovisioned). The
-- controller reads tenant-row + user-row `cost_micros` against the daily caps BEFORE any provider/STT
-- call (B-R20, D17); over-ceiling → refusal, no call made.

CREATE TABLE cost_ledger (
  id              UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id       UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  scope           TEXT NOT NULL,                       -- 'tenant' | 'user'
  user_id         UUID REFERENCES users(id) ON DELETE CASCADE,  -- NULL when scope='tenant'
  day             DATE NOT NULL,                        -- UTC day bucket
  input_tokens    BIGINT NOT NULL DEFAULT 0,
  output_tokens   BIGINT NOT NULL DEFAULT 0,
  cache_read_tokens  BIGINT NOT NULL DEFAULT 0,
  cache_write_tokens BIGINT NOT NULL DEFAULT 0,
  reasoning_tokens   BIGINT NOT NULL DEFAULT 0,         -- the 5th CanonicalUsage field; accrued by S3-R52, summed by the D17 cost cap (a real ledger column, not telemetry-only)
  cost_micros     BIGINT NOT NULL DEFAULT 0,            -- accumulated spend, micro-USD (D17, B-R10)
  provider_breakdown JSONB NOT NULL DEFAULT '{}',       -- {provider: micros} for the day
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  -- exactly one tenant-row and one row per user per day
  UNIQUE (tenant_id, scope, user_id, day)
);
CREATE INDEX cost_ledger_day_idx ON cost_ledger (tenant_id, day DESC);
CREATE TRIGGER cost_ledger_touch BEFORE UPDATE ON cost_ledger
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(cost_ledger)
ALTER TABLE cost_ledger ENABLE ROW LEVEL SECURITY;
ALTER TABLE cost_ledger FORCE  ROW LEVEL SECURITY;
CREATE POLICY cost_ledger_tenant_isolation ON cost_ledger
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('cost_ledger');
