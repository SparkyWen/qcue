-- QCue REC-R2/REC-D2 — the recall conversation HEADER table (thread list + titles). One row per
-- recall thread (`id` = the thread/session UUID used as `messages.session_id`). Kept separate from
-- `messages` so the drawer can ORDER BY updated_at without a GROUP-BY-on-messages. RLS block mirrors
-- `messages`/`session_kv` (FORCE row security; the `touch_updated_at` trigger system-sets updated_at).

CREATE TABLE conversations (
  id          UUID PRIMARY KEY,                    -- the thread/session UUID (== messages.session_id)
  tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title       TEXT NOT NULL,                        -- derived from the first user message (REC-D3)
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()    -- touched per turn (REC-R2)
);
-- the hot read: a tenant's conversations newest-first (the drawer list, REC-R3).
CREATE INDEX conversations_recent_idx ON conversations (tenant_id, updated_at DESC);
CREATE TRIGGER conversations_touch BEFORE UPDATE ON conversations
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(conversations)
ALTER TABLE conversations ENABLE ROW LEVEL SECURITY;
ALTER TABLE conversations FORCE  ROW LEVEL SECURITY;
CREATE POLICY conversations_tenant_isolation ON conversations
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('conversations');
