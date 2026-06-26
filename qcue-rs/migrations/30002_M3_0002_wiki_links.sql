-- QCue Appendix B §4.9 `wiki_links` (the link graph — O(1) dead-link / orphan; pitfall #12).
-- Table/indexes copied VERBATIM from the appendix; the `-- RLS(wiki_links)` shorthand is expanded to
-- the §4 block (the trailing GRANT routed through `_grant_app()`, skipped when the app role is
-- unprovisioned). The `wiki_page_type` enum is declared once in the §2.1 prelude (00000).
--
-- B-R16 (graph scans are pure SQL, no file reads): dead links =
--   SELECT … FROM wiki_links WHERE tenant_id=$1 AND target_page_id IS NULL  (wiki_links_dead_idx);
-- orphans =
--   SELECT p.id FROM wiki_pages p WHERE p.tenant_id=$1 AND p.deleted_at IS NULL AND NOT EXISTS
--     (SELECT 1 FROM wiki_links l WHERE l.tenant_id=$1 AND l.target_page_id=p.id)  (wiki_links_incoming_idx).
-- Both are index-only + tenant-scoped — the lint scanners never re-read markdown.

CREATE TABLE wiki_links (
  id             UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id      UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  src_page_id    UUID NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
  target_slug    TEXT NOT NULL,                       -- the [[wikilink]] target as written
  target_type    wiki_page_type,                      -- inferred folder, NULL if unqualified
  target_page_id UUID REFERENCES wiki_pages(id) ON DELETE SET NULL,  -- NULL ⇒ DEAD LINK
  display        TEXT,                                -- optional [[slug|Display]] text
  created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- dead-link scan: O(rows) on a covering index, no body reads (pitfall #12)
CREATE INDEX wiki_links_dead_idx    ON wiki_links (tenant_id, target_page_id) WHERE target_page_id IS NULL;
-- orphan scan: pages with zero incoming links = wiki_pages LEFT JOIN this on target_page_id IS NULL
CREATE INDEX wiki_links_incoming_idx ON wiki_links (tenant_id, target_page_id);
CREATE INDEX wiki_links_outgoing_idx ON wiki_links (tenant_id, src_page_id);
CREATE UNIQUE INDEX wiki_links_edge_uniq ON wiki_links (tenant_id, src_page_id, target_slug);
-- RLS(wiki_links)
ALTER TABLE wiki_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE wiki_links FORCE  ROW LEVEL SECURITY;
CREATE POLICY wiki_links_tenant_isolation ON wiki_links
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('wiki_links');
