-- QCue Appendix B §4.21 `sync_ops` (CRDT op-log — D6, M5; `clariose-priorart.md` filesystem-as-bus).
-- The encrypted op-log hub: the device holds the working markdown; edits flow up as CRDT ops; the
-- server materializes markdown + the link index (Master §7). Append-only, ordered by a hybrid-logical
-- clock so multi-device merges are conflict-free.
--
-- Table/indexes copied VERBATIM from §4.21; the `-- RLS(sync_ops)` shorthand is expanded to the §4
-- block (the trailing GRANT routed through `_grant_app()`, skipped when the app role is unprovisioned).
--
-- B-R21 (op-log is idempotent + totally-ordered): the unique `(tenant_id, device_id, site_id,
-- hlc_lamport)` makes a re-sent op a no-op INSERT-conflict; the `(wall_ms, lamport, site_id)` order is
-- total so two devices materialize identical markdown (D6).
-- B-R11 (secrets never in content tables): provider keys NEVER reach `sync_ops.op` — the Rust
-- persistence boundary redacts before INSERT (asserted by test).

CREATE TABLE sync_ops (
  id           UUID PRIMARY KEY DEFAULT uuidv7(),
  tenant_id    UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  device_id    UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
  -- hybrid logical clock: (wall_ms, lamport, site_id) gives a total order across devices (D6)
  hlc_wall_ms  BIGINT NOT NULL,                        -- physical component
  hlc_lamport  BIGINT NOT NULL,                        -- logical counter
  site_id      BIGINT NOT NULL,                        -- devices.site_id tiebreaker
  entity_kind  TEXT NOT NULL,                          -- 'wiki_page'|'idea'|'memory_file'
  entity_ref   TEXT NOT NULL,                          -- slug / id the op targets
  op           JSONB NOT NULL,                         -- the CRDT op payload (opaque bag, B-R8)
  applied      BOOLEAN NOT NULL DEFAULT false,         -- has the server materialized it yet
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- total-order replay per tenant (leading tenant_id, then the HLC tuple)
CREATE INDEX sync_ops_order_idx ON sync_ops (tenant_id, hlc_wall_ms, hlc_lamport, site_id);
-- pull-since for a reconnecting device
CREATE INDEX sync_ops_unapplied_idx ON sync_ops (tenant_id, applied, hlc_wall_ms) WHERE NOT applied;
CREATE UNIQUE INDEX sync_ops_idem_uniq ON sync_ops (tenant_id, device_id, site_id, hlc_lamport);  -- idempotent re-send
-- RLS(sync_ops)
ALTER TABLE sync_ops ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_ops FORCE  ROW LEVEL SECURITY;
CREATE POLICY sync_ops_tenant_isolation ON sync_ops
  USING       (tenant_id = app_tenant())
  WITH CHECK  (tenant_id = app_tenant());
SELECT _grant_app('sync_ops');
