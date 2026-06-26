-- QCue Appendix B §4.13 `messages` (harness superset transcript) + §6.1 messages search columns.
-- Table/indexes/trigger copied VERBATIM; the `-- RLS(messages)` shorthand is expanded to the §4
-- block (GRANT via `_grant_app()`). The §6.1 generated `search_tsv` + GIN + trgm indexes are
-- declared with the base table (built from `content` only — reasoning/provider_data excluded, B-R23).

CREATE TABLE messages (
  id            UUID NOT NULL DEFAULT uuidv7(),       -- stable id (B-R1)
  seq           BIGSERIAL,                            -- monotonic in-session order (no clock skew)
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  session_id    UUID NOT NULL,                        -- Thread id (idea/wiki/recall/dream session)
  user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role          msg_role NOT NULL,                    -- system|user|assistant|tool
  content       TEXT,
  tool_call_id  TEXT,                                 -- for role='tool'
  tool_name     TEXT,
  tool_calls    JSONB,                                -- assistant tool calls (arguments are JSON STRINGS — pitfall, byte-stable)
  finish_reason TEXT,
  reasoning     TEXT,                                 -- shown collapsed-by-default in app (D18)
  -- ONE opaque bag for ALL provider-specific replay fields: reasoning_content (DeepSeek/Kimi, pitfall #7),
  -- reasoning_details / thinking signatures (Anthropic, opaque byte-stable, pitfall #8), provider_data.
  -- NEVER vendor-native top-level columns (pitfall #1). Unknown keys allowed here ONLY (B-R8).
  provider_data JSONB,
  provider      TEXT,                                 -- which provider/model produced an assistant turn
  model         TEXT,
  -- normalized usage for billing (the 4 vendor cache-token shapes folded to one; hermes §9, pitfall #19)
  usage         JSONB,                                -- {input, output, cache_read, cache_write, reasoning}
  request_id    TEXT,                                 -- dedup usage by request_id (pitfall #19), x-request-id
  active        BOOLEAN NOT NULL DEFAULT true,        -- rewind / soft-delete (B-R9)
  is_untrusted  BOOLEAN NOT NULL DEFAULT false,       -- tail-only untrusted content marker (RKM §7 #3)
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (id)
);
-- the hot read: a session's active transcript in order, tenant-led (B-R3)
CREATE INDEX messages_session_idx ON messages (tenant_id, session_id, seq) WHERE active;
-- usage dedup / cost rollup join key
CREATE INDEX messages_request_idx ON messages (tenant_id, request_id) WHERE request_id IS NOT NULL;
-- RLS(messages)
ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE messages FORCE  ROW LEVEL SECURITY;
CREATE POLICY messages_tenant_isolation ON messages
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('messages');

-- §6.1 messages: search content only (reasoning/provider_data excluded — never index secrets, B-R11)
-- NOTE: Appendix B writes `unaccent(...)`; Postgres requires an IMMUTABLE expression for a STORED
-- generated column, so this uses the `immutable_unaccent()` wrapper from the prelude (same folding).
ALTER TABLE messages ADD COLUMN search_tsv tsvector
  GENERATED ALWAYS AS (to_tsvector('simple', immutable_unaccent(coalesce(content,'')))) STORED;
CREATE INDEX message_search_gin ON messages USING gin (tenant_id, search_tsv) WHERE active;
CREATE INDEX message_trgm_gin   ON messages USING gin (tenant_id, content gin_trgm_ops) WHERE active;
