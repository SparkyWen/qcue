-- QCue Appendix B §4.2 `users`, §4.3 `oauth_identities`, §4.4 `sessions`, §4.5 `devices`
-- (the `M0_0002_users` migration per §11.1). Tables copied VERBATIM from the appendix. Two
-- adaptations, both mechanical:
--   1. table ORDER inside the file satisfies FK dependencies (users → devices → sessions →
--      oauth_identities) since `sessions.device_id REFERENCES devices(id)`; Appendix B lists them
--      §4.2→§4.5 (sessions before devices) which is presentation order, not executable order.
--   2. the `-- RLS(t)` shorthand is expanded to the §4 block VERBATIM, with the trailing
--      `GRANT ... TO qcue_app` routed through `_grant_app()` so it is skipped when the app role is
--      not provisioned (sqlx test fixture).
-- `email`/`oauth_identities.email` use CITEXT (Appendix B §4.2/§4.3); the prelude enables it here.
CREATE EXTENSION IF NOT EXISTS citext;

-- §4.2 users
CREATE TABLE users (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  email         CITEXT NOT NULL UNIQUE,              -- global unique (B-R6 exception); CITEXT = case-insensitive
  password_hash TEXT,                                -- argon2id; NULL when only social auth (D11)
  role          TEXT NOT NULL DEFAULT 'owner',       -- D8 solo: 'owner'; future teams add more
  display_name  TEXT,
  is_active     BOOLEAN NOT NULL DEFAULT true,
  per_user_daily_cost_cap_micros BIGINT NOT NULL DEFAULT 5000000,  -- D17 per-user/day ceiling
  last_login_at TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX users_tenant_idx ON users (tenant_id, id);   -- leading tenant_id (B-R3)
CREATE TRIGGER users_touch BEFORE UPDATE ON users
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(users)
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE users FORCE  ROW LEVEL SECURITY;
CREATE POLICY users_tenant_isolation ON users
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('users');

-- §4.5 devices (created before sessions: sessions.device_id REFERENCES devices(id))
CREATE TABLE devices (
  id            UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id     UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id       UUID NOT NULL REFERENCES users(id)   ON DELETE CASCADE,
  platform      TEXT NOT NULL,                        -- 'ios' | 'android'
  display_name  TEXT,
  -- CRDT identity: a stable site-id for HLC/Lamport ordering (D6, sync_ops §4.10)
  site_id       BIGINT NOT NULL,                      -- small int per device, unique within tenant
  last_seen_at  TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, site_id)
);
CREATE INDEX devices_tenant_user_idx ON devices (tenant_id, user_id);
CREATE TRIGGER devices_touch BEFORE UPDATE ON devices
  FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
-- RLS(devices)
ALTER TABLE devices ENABLE ROW LEVEL SECURITY;
ALTER TABLE devices FORCE  ROW LEVEL SECURITY;
CREATE POLICY devices_tenant_isolation ON devices
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('devices');

-- §4.4 sessions
CREATE TABLE sessions (
  id           UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id      UUID NOT NULL REFERENCES users(id)   ON DELETE CASCADE,
  jti          UUID NOT NULL,                        -- JWT id for revocation
  device_id    UUID REFERENCES devices(id) ON DELETE SET NULL,
  user_agent   TEXT,
  ip_hash      TEXT,                                 -- hashed, never raw IP (privacy)
  issued_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at   TIMESTAMPTZ NOT NULL,
  revoked_at   TIMESTAMPTZ,
  UNIQUE (tenant_id, jti)
);
CREATE INDEX sessions_tenant_user_idx ON sessions (tenant_id, user_id, issued_at DESC);
CREATE INDEX sessions_active_idx ON sessions (tenant_id, expires_at) WHERE revoked_at IS NULL;
-- RLS(sessions)
ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE sessions FORCE  ROW LEVEL SECURITY;
CREATE POLICY sessions_tenant_isolation ON sessions
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('sessions');

-- §4.3 oauth_identities (Apple / Google social auth — D11)
CREATE TABLE oauth_identities (
  id         UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id  UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id    UUID NOT NULL REFERENCES users(id)   ON DELETE CASCADE,
  provider   TEXT NOT NULL,                         -- 'apple' | 'google'
  subject    TEXT NOT NULL,                         -- the IdP's stable subject id
  email      CITEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (provider, subject)                        -- global (B-R6 exception): one IdP identity → one user
);
CREATE INDEX oauth_user_idx ON oauth_identities (tenant_id, user_id);
-- RLS(oauth_identities)
ALTER TABLE oauth_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE oauth_identities FORCE  ROW LEVEL SECURITY;
CREATE POLICY oauth_identities_tenant_isolation ON oauth_identities
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('oauth_identities');
