-- QCue Appendix B §4.7 `ideas` (the capture log — append-only; D10, Master §3) + §6.1 ideas search.
-- The capture feed: persisted BEFORE any LLM call so a crash mid-ingest still records intent (Master
-- §3 persistence law, RKM §6). Table/indexes/trigger + the full RLS block copied VERBATIM from the
-- appendix (§4.7 is the one table shown with its RLS block in full). The §6.1 generated `search_tsv`
-- column + GIN + trgm indexes are declared with the base table (built from `body`). The `idea_kind`/
-- `ingest_state` enums are declared once in the §2.1 prelude (00000).
--
-- NOTE: the appendix §6.1 writes `unaccent(...)`; Postgres requires an IMMUTABLE expression for a
-- STORED generated column, so this uses the `immutable_unaccent()` wrapper from the prelude (same
-- diacritic folding). The trailing per-table GRANT is routed through `_grant_app()` (skipped when the
-- app role is unprovisioned) instead of the bare `GRANT … TO qcue_app` the verbatim block emits.

CREATE TABLE ideas (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id       UUID NOT NULL REFERENCES users(id)   ON DELETE CASCADE,
  kind          idea_kind NOT NULL DEFAULT 'text',
  body          TEXT NOT NULL DEFAULT '',             -- transcript for voice; text for text; caption for clip
  audio_ref     TEXT,                                 -- object-store key of raw audio (voice); NULL otherwise
  source_url    TEXT,                                 -- for kind='clip' (share-sheet / web clipper, S5)
  log_ref       TEXT NOT NULL,                        -- object-store JSONL key = canonical truth (§9)
  transcript_provider TEXT,                           -- which STT provider produced body (voice); NULL else
  ingest_state  ingest_state NOT NULL DEFAULT 'pending',
  ingest_job_id UUID,                                 -- FK-soft to jobs.id (set when enqueued)
  -- wrapped-untrusted marker: captures are untrusted input (RKM §7 #1). origin recorded for fencing.
  origin        TEXT NOT NULL DEFAULT 'capture',      -- 'capture'|'share'|'web'|'import'|'voice'
  captured_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
  active        BOOLEAN NOT NULL DEFAULT true,        -- soft-delete (B-R9)
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- feed read (reverse-chron) + ingest worker scan, both lead with tenant_id (B-R3)
CREATE INDEX ideas_feed_idx    ON ideas (tenant_id, user_id, captured_at DESC) WHERE active;
CREATE INDEX ideas_pending_idx ON ideas (tenant_id, ingest_state, captured_at) WHERE ingest_state = 'pending';
CREATE TRIGGER ideas_touch BEFORE UPDATE ON ideas
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();

-- Full RLS block, shown once in full (every other table uses the -- RLS(t) shorthand):
ALTER TABLE ideas ENABLE ROW LEVEL SECURITY;
ALTER TABLE ideas FORCE  ROW LEVEL SECURITY;
CREATE POLICY ideas_tenant_isolation ON ideas
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('ideas');

-- §6.1 ideas: search the body. unaccent for diacritic folding; 'simple' config so we don't stem CJK away.
ALTER TABLE ideas ADD COLUMN search_tsv tsvector
  GENERATED ALWAYS AS (to_tsvector('simple', immutable_unaccent(coalesce(body,'')))) STORED;
CREATE INDEX idea_search_gin ON ideas USING gin (tenant_id, search_tsv);   -- tenant-led via btree_gin (B-R3)
-- trigram index for CJK / substring on the same body
CREATE INDEX idea_trgm_gin   ON ideas USING gin (tenant_id, body gin_trgm_ops);
