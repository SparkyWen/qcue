-- QCue cloud-sync — idempotent capture (server-side dedup by tenant + Idempotency-Key).
-- Additive, idempotent (IF NOT EXISTS): adds an `idempotency_key` column to the `ideas` table
-- (Appendix B §4.7) + a per-tenant partial UNIQUE index so a retried POST /v1/capture carrying the
-- same `Idempotency-Key` header dedups to the existing row (ON CONFLICT DO NOTHING + re-select) rather
-- than inserting/enqueuing a duplicate. NULL keys are excluded from the uniqueness (legacy/keyless
-- captures stay unconstrained).
ALTER TABLE ideas ADD COLUMN IF NOT EXISTS idempotency_key TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS ideas_tenant_idem_uidx
  ON ideas (tenant_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
