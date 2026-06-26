-- QCue Appendix B §4.6 `provider_credentials` (BYOK vault — envelope-encrypted; D1/D9, RKM §7 #6).
-- Table/indexes/trigger copied VERBATIM; the `-- RLS(provider_credentials)` shorthand is expanded to
-- the §4 block, with the trailing GRANT routed through `_grant_app()` (skipped when the app role is
-- unprovisioned). The 3-state pool machine (ok|exhausted|dead) is persisted here as source of truth.

CREATE TABLE provider_credentials (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  provider      TEXT NOT NULL,                        -- 'openai'|'anthropic'|'gemini'|'deepseek'|'kimi'|'qwen'|'openrouter'|... (D7)
  label         TEXT,                                 -- user-facing name for this key in the vault UI
  priority      INT NOT NULL DEFAULT 0,               -- pool ordering (fill_first / least_used use this)

  -- ── Envelope encryption columns (the secret NEVER appears in plaintext anywhere else) ──
  key_ciphertext BYTEA NOT NULL,                      -- AES-256-GCM ciphertext of the provider API key
  key_nonce      BYTEA NOT NULL,                      -- 96-bit GCM nonce
  key_tag        BYTEA NOT NULL,                      -- GCM auth tag
  dek_wrapped    BYTEA NOT NULL,                      -- the per-tenant DEK, wrapped by the KMS KEK
  kek_id         TEXT  NOT NULL,                      -- which KMS KEK version wrapped the DEK (rotation)
  key_hint       TEXT  NOT NULL,                      -- last-4 / fingerprint for UI + api_key_hint (pitfall #4)

  -- ── 3-state credential pool machine (RKM §3.4; pitfall #3 DEAD vs EXHAUSTED) ──
  status         cred_status NOT NULL DEFAULT 'ok',
  cooldown_until TIMESTAMPTZ,                          -- set when status='exhausted' (401→+5min, 429→+1h, default→+1h)
  last_error_code   INT,                               -- HTTP status of last failure
  last_error_reason TEXT,                              -- FailoverReason discriminant string
  request_count  BIGINT NOT NULL DEFAULT 0,            -- least_used strategy ordering
  dead_at        TIMESTAMPTZ,                          -- terminal-auth (token_revoked/invalid_grant) → DEAD; pruned after 24h

  created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- one logical key per (tenant, provider, hint); multiple keys per provider allowed (pool)
CREATE UNIQUE INDEX provider_creds_uniq ON provider_credentials (tenant_id, provider, key_hint);
-- pool selection hot path: pick OK or cooled-down keys for a (tenant, provider), least-used first
CREATE INDEX provider_creds_pool_idx ON provider_credentials (tenant_id, provider, status, cooldown_until, request_count);
CREATE TRIGGER provider_creds_touch BEFORE UPDATE ON provider_credentials
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(provider_credentials)
ALTER TABLE provider_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE provider_credentials FORCE  ROW LEVEL SECURITY;
CREATE POLICY provider_credentials_tenant_isolation ON provider_credentials
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('provider_credentials');
