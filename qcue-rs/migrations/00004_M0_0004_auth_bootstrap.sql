-- QCue S3 — the audited auth-bootstrap RLS-exempt SEAM (Appendix B §4.2 / B-R20 / open item §13).
--
-- WHY: login + magic-link + the JWT revocation check must look up a user/session by a GLOBAL key
-- (email / jti) BEFORE any tenant context exists — "login happens before any tenant context exists"
-- (§4.2). Appendix B specs this as a narrowly-scoped RLS-exempt path performed "as the migrator-
-- equivalent auth role with RLS bypassed for exactly that one query". In the canonical deployment
-- that path is a dedicated BYPASSRLS auth role; this environment provisions only a single non-
-- superuser, non-BYPASSRLS role and FORCE ROW LEVEL SECURITY blocks even the table owner, so the
-- seam is expressed instead as ADDITIVE, SELECT-ONLY permissive policies that fire ONLY when no
-- tenant is bound (`app_tenant() IS NULL`).
--
-- SECURITY ENVELOPE (why this does NOT weaken tenant isolation, B-R26):
--   * Permissive policies are OR'd, so these only WIDEN SELECT — never INSERT/UPDATE/DELETE, which
--     stay governed solely by the existing `*_tenant_isolation` policy (tenant-scoped writes).
--   * They fire ONLY when `app_tenant()` IS NULL — i.e. the pre-auth bootstrap read. EVERY
--     authenticated request runs `SET LOCAL app.tenant_id` first (the extractor), so for all real
--     traffic `app_tenant()` is non-NULL and the original `tenant_id = app_tenant()` predicate is
--     the sole gate → cross-tenant SELECT stays impossible.
--   * Scoped to the three auth tables ONLY (users / sessions / oauth_identities). Content tables
--     (ideas / wiki_* / messages / …) keep their unmodified FORCE-RLS isolation — the bootstrap
--     role can never read tenant content.
--   * This is the single documented, audited exception (§4.2, B-R20); it is the literal "open item"
--     Appendix B §13 left for S3 to spec.

CREATE POLICY users_auth_bootstrap_read ON users
  FOR SELECT
  USING (app_tenant() IS NULL);

CREATE POLICY sessions_auth_bootstrap_read ON sessions
  FOR SELECT
  USING (app_tenant() IS NULL);

CREATE POLICY oauth_identities_auth_bootstrap_read ON oauth_identities
  FOR SELECT
  USING (app_tenant() IS NULL);
