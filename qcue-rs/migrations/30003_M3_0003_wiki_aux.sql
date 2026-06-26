-- QCue Appendix B §4.10 `wiki_log` (chronological audit; wiki-engine.md §5) + §4.11
-- `wiki_contradictions` (wiki-engine.md §7) + §4.12 `wiki_schema` (the 3rd Karpathy layer —
-- human-approved; D16, wiki-engine.md §8; its `suggestions` JSONB holds the pending co-evolution
-- proposals, NEVER auto-applied) + §4.17 `memory_files` (curated MEMORY.md / USER.md —
-- frozen-snapshot recall; D-recall, Master §5).
--
-- Tables/indexes/triggers copied VERBATIM from the appendix; each `-- RLS(t)` shorthand is expanded to
-- the §4 block (the trailing GRANT routed through `_grant_app()`, skipped when the app role is
-- unprovisioned). The `contra_status` enum is declared once in the §2.1 prelude (00000).

-- ============================ §4.10 wiki_log ============================
CREATE TABLE wiki_log (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  op            TEXT NOT NULL,                         -- 'ingest'|'merge'|'lint_fix'|'dream'|'contradiction'
  title         TEXT NOT NULL,
  source_id     UUID REFERENCES ideas(id) ON DELETE SET NULL,
  created_pages UUID[] NOT NULL DEFAULT '{}',
  updated_pages UUID[] NOT NULL DEFAULT '{}',
  detail        JSONB NOT NULL DEFAULT '{}',
  logged_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX wiki_log_tenant_idx ON wiki_log (tenant_id, logged_at DESC);
-- RLS(wiki_log)
ALTER TABLE wiki_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_log FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_log_tenant_isolation ON wiki_log
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_log');

-- ============================ §4.11 wiki_contradictions ============================
CREATE TABLE wiki_contradictions (
  id             UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  claim          TEXT NOT NULL,
  source_page_id UUID REFERENCES wiki_pages(id) ON DELETE SET NULL,
  contradicted_by UUID REFERENCES wiki_pages(id) ON DELETE SET NULL,
  resolution     TEXT,
  status         contra_status NOT NULL DEFAULT 'detected',
  detected_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  resolved_at    TIMESTAMPTZ,
  created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX wiki_contra_open_idx ON wiki_contradictions (tenant_id, status, detected_at)
  WHERE status IN ('detected','review_ok','pending_fix');
CREATE TRIGGER wiki_contra_touch BEFORE UPDATE ON wiki_contradictions
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(wiki_contradictions)
ALTER TABLE wiki_contradictions ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_contradictions FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_contradictions_tenant_isolation ON wiki_contradictions
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_contradictions');

-- ============================ §4.12 wiki_schema ============================
CREATE TABLE wiki_schema (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  version     INT NOT NULL DEFAULT 1,
  body_ref    TEXT NOT NULL,                          -- vault key of schema/config.md (canonical)
  -- pending co-evolution suggestions: appended by ingest, NEVER auto-applied (D16)
  suggestions JSONB NOT NULL DEFAULT '[]',
  active      BOOLEAN NOT NULL DEFAULT true,          -- one active version per tenant
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX wiki_schema_active_uniq ON wiki_schema (tenant_id) WHERE active;
CREATE TRIGGER wiki_schema_touch BEFORE UPDATE ON wiki_schema
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(wiki_schema)
ALTER TABLE wiki_schema ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_schema FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_schema_tenant_isolation ON wiki_schema
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_schema');

-- ============================ §4.17 memory_files ============================
CREATE TABLE memory_files (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,                          -- 'MEMORY' | 'USER'
  body_ref    TEXT NOT NULL,                          -- vault key of MEMORY.md / USER.md (canonical)
  char_cap    INT NOT NULL,                           -- the storage column for the curated-memory char cap; defaults ~2200 (MEMORY) / ~1375 (USER) are SEEDED by S2-R32 (Master §5)
  content_hash TEXT NOT NULL,                         -- of the frozen snapshot (cache-attribution, claude-cc §1.5)
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, kind)
);
CREATE TRIGGER memory_files_touch BEFORE UPDATE ON memory_files
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(memory_files)
ALTER TABLE memory_files ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_files FORCE  ROW LEVEL SECURITY;
CREATE POLICY memory_files_tenant_isolation ON memory_files
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('memory_files');
