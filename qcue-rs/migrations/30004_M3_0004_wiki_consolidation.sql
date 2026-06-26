-- QCue Appendix B §4.16 `wiki_consolidation` (Auto-Dream lock-as-clock; D15, Master §6, Appendix A).
-- Faithful port of Claude Code 2.1.88's `.consolidate-lock` (file mtime = lastConsolidatedAt, body =
-- PID, HOLDER_STALE_MS=1h, dead-PID reclaim, two-writer last-wins) into ONE row that is *both* the
-- lease and the clock (Master §6). Rollback-on-failure rewinds the clock (the scan-throttle is the
-- backoff). One row per tenant.
--
-- Table/trigger copied VERBATIM from the appendix; the `-- RLS(wiki_consolidation)` shorthand is
-- expanded to the §4 block (the trailing GRANT routed through `_grant_app()`, skipped when the app
-- role is unprovisioned). `tenant_id` IS the PK (one row/tenant) and satisfies B-R2.
--
-- B-R19 (clock rewinds on failure): a failed/rolled-back Dream leaves `last_consolidated_at` UNCHANGED
-- and frees the lease; the clock only advances on a committed successful consolidation. The CACHED
-- `sessions_since_last` is convenience/telemetry ONLY — the LIVE store.captures_since COUNT (A-R10) is
-- AUTHORITATIVE for the minSessions gate.

CREATE TABLE wiki_consolidation (
  tenant_id            UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,  -- one row/tenant; IS the tenant_id (B-R2)
  -- ── lock-as-clock ──
  last_consolidated_at TIMESTAMPTZ,                   -- the CLOCK: gates minHours since last Dream
  holder               TEXT,                          -- the LEASE holder (worker id / PID); NULL = free
  lease_expires        TIMESTAMPTZ,                   -- HOLDER_STALE_MS analog; past ⇒ stale, reclaimable
  -- ── scan-throttle + gate ladder bookkeeping ──
  last_scan_at         TIMESTAMPTZ,                   -- scan-throttle (10min) cheapest-after-enabled gate
  sessions_since_last  INT NOT NULL DEFAULT 0,        -- CACHED convenience/telemetry counter ONLY; the LIVE store.captures_since COUNT (App. A A-R10) is AUTHORITATIVE for the minSessions gate
  last_dream_run_id    UUID,                          -- soft-ref to the jobs row of the last dream
  rollback_count       INT NOT NULL DEFAULT 0,        -- diagnostics: how often the clock was rewound
  updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER wiki_consolidation_touch BEFORE UPDATE ON wiki_consolidation
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(wiki_consolidation)
ALTER TABLE wiki_consolidation ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_consolidation FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_consolidation_tenant_isolation ON wiki_consolidation
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_consolidation');
