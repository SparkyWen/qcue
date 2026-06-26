-- QCue Appendix B §4.8 `wiki_pages` (Karpathy wiki structure index — D2; wiki-engine.md §1,§10) +
-- §6.1 wiki search (generated tsvector + GIN + trgm). Table/indexes/trigger copied VERBATIM from the
-- appendix; the `-- RLS(wiki_pages)` shorthand is expanded to the §4 block (the trailing GRANT routed
-- through `_grant_app()`, skipped when the app role is unprovisioned). The `wiki_page_type` enum is
-- declared once in the §2.1 prelude (00000) — NOT redeclared here.
--
-- Dual representation: the markdown body is the content source-of-truth (vault/object store at
-- `body_ref`); Postgres mirrors frontmatter + slug + dates + summary + `char_len` for fast
-- retrieval/lint without parsing every file (pitfall #12). `char_len` is SYSTEM-set by the S2-R49
-- write-gate, never LLM-set (B-R7); it enables the pure-SQL empty-page lint with no body reads.
--
-- NOTE: the appendix §6.1 writes `unaccent(...)` directly; Postgres requires an IMMUTABLE expression
-- for a STORED generated column, so this uses the `immutable_unaccent()` wrapper from the prelude
-- (same diacritic folding). The verbatim §6.1 wiki vector ALSO calls `array_to_string(aliases,' ')`,
-- which Postgres ships STABLE (not IMMUTABLE) and therefore likewise rejects in a STORED generated
-- column; this migration adds a minimal `immutable_array_to_string(text[])` wrapper (same join
-- semantics, ' '-delimited) — the identical adaptation pattern as `immutable_unaccent`. The trailing
-- per-table GRANT routes through `_grant_app()` instead of the bare `GRANT … TO qcue_app` the verbatim
-- RLS block emits.

-- IMMUTABLE wrapper for the §6.1 wiki search vector's `array_to_string(aliases,' ')` (text[] → text,
-- space-joined). array_to_string ships STABLE; a STORED generated column requires IMMUTABLE. This
-- preserves the exact join semantics (NULL elements dropped) while satisfying the immutability rule.
CREATE OR REPLACE FUNCTION immutable_array_to_string(text[]) RETURNS text AS $$
  SELECT array_to_string($1, ' ');
$$ LANGUAGE sql IMMUTABLE;

CREATE TABLE wiki_pages (
  id          UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  type        wiki_page_type NOT NULL,
  slug        TEXT NOT NULL,                          -- e.g. 'tsinghua-university'
  title       TEXT NOT NULL,
  aliases     TEXT[] NOT NULL DEFAULT '{}',           -- alias-aware dedup + index (wiki-engine §3)
  tags        TEXT[] NOT NULL DEFAULT '{}',
  summary     TEXT NOT NULL DEFAULT '',               -- explicit column, set at write time (not re-derived; §5 pitfall)
  char_len    INT NOT NULL DEFAULT 0,                  -- char length of the .md body; SYSTEM-set by the S2-R49 write-gate, never LLM-set (B-R7). Enables the pure-SQL empty-page lint without reading bodies (pitfall #12)
  body_ref    TEXT NOT NULL,                          -- vault/object-store key of the .md body (canonical content)
  source_ids  UUID[] NOT NULL DEFAULT '{}',           -- ideas.id[] that contributed (provenance)
  reviewed    BOOLEAN NOT NULL DEFAULT false,         -- human-verified → protected from auto-rewrite (wiki-engine §4)
  -- wiki created/updated are SYSTEM-set, never LLM-set (B-R7, RKM §4, schema-manager.ts:98-102)
  created     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated     TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at  TIMESTAMPTZ,                            -- soft-delete for reversible Dream merges (B-R9, pitfall #18)
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX wiki_pages_slug_uniq ON wiki_pages (tenant_id, type, slug) WHERE deleted_at IS NULL;
CREATE INDEX wiki_pages_tenant_idx ON wiki_pages (tenant_id, updated DESC) WHERE deleted_at IS NULL;
-- alias/tag lookup for the index-first selector (GIN over arrays), tenant-led via btree_gin
CREATE INDEX wiki_pages_aliases_gin ON wiki_pages USING gin (tenant_id, aliases) WHERE deleted_at IS NULL;
CREATE INDEX wiki_pages_tags_gin    ON wiki_pages USING gin (tenant_id, tags)    WHERE deleted_at IS NULL;
-- provenance reverse-lookup: which pages did this idea touch
CREATE INDEX wiki_pages_sources_gin ON wiki_pages USING gin (tenant_id, source_ids);
-- empty-page lint: pure-SQL scan for near-empty pages via the system-set char_len, no body reads (pitfall #12)
CREATE INDEX wiki_pages_empty_idx ON wiki_pages (tenant_id, char_len) WHERE deleted_at IS NULL;
CREATE TRIGGER wiki_pages_touch BEFORE UPDATE ON wiki_pages
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(wiki_pages)
ALTER TABLE wiki_pages ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_pages FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_pages_tenant_isolation ON wiki_pages
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_pages');

-- §6.1 wiki_pages: search title + aliases + summary (the index-first selector substrate; body stays in vault)
ALTER TABLE wiki_pages ADD COLUMN search_tsv tsvector
  GENERATED ALWAYS AS (
    to_tsvector('simple',
      immutable_unaccent(coalesce(title,'') || ' ' || immutable_array_to_string(aliases) || ' ' || coalesce(summary,'')))
  ) STORED;
CREATE INDEX wiki_search_gin ON wiki_pages USING gin (tenant_id, search_tsv) WHERE deleted_at IS NULL;
CREATE INDEX wiki_trgm_gin   ON wiki_pages USING gin (tenant_id, title gin_trgm_ops) WHERE deleted_at IS NULL;
