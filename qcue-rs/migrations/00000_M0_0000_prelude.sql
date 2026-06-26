-- QCue Appendix B §2.1 (shared SQL prelude) + §4.0 (roles & grants). Copied VERBATIM from
-- docs/superpowers/specs/2026-06-13-qcue-appendix-b-data-model-and-migrations.md, with the ONLY
-- adaptation being the role-creation block (Appendix B §4.0): the verbatim DDL uses psql client
-- variables (:'app_pw') and assumes a CREATEROLE migrator. Under sqlx the migrations execute as the
-- DB owner (which may lack CREATEROLE), so the role creation is wrapped in a permission-tolerant DO
-- block — a no-op where roles cannot be created, the full role model where they can. The table/RLS/
-- index DDL below is otherwise byte-for-byte the appendix.

-- ============================ §2.1 Shared SQL prelude ============================
-- Extensions (idempotent). pgcrypto for gen_random_uuid fallback; pg_trgm for CJK/substring;
-- unaccent for diacritic-folded FTS. pgvector is RESERVED but NOT created until M6 (NG1).
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS unaccent;
CREATE EXTENSION IF NOT EXISTS btree_gin;   -- lets a GIN index lead with tenant_id (B-R3) alongside tsvector

-- UUID v7 generator (time-ordered). If PG18+ ships uuidv7() natively, this becomes a thin alias.
-- Until then, a SQL/PLpgSQL implementation; deterministic, no extension beyond pgcrypto.
CREATE OR REPLACE FUNCTION uuidv7() RETURNS uuid AS $$
DECLARE
  unix_ts_ms BIGINT := (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT;
  rand_a INT := (random() * 4095)::INT;          -- 12 random bits
  uuid_bytes BYTEA;
BEGIN
  -- NOTE: Appendix B writes the byte values as bare bigint expressions; Postgres 16 has no
  -- set_byte(bytea,int,bigint) overload, so each value is cast ::int (the only change; values are
  -- masked to 0..255 so the cast is lossless). Verbatim semantics, real-Postgres-compatible.
  uuid_bytes := set_byte(set_byte(set_byte(set_byte(set_byte(set_byte(
    gen_random_bytes(16),
    0, ((unix_ts_ms >> 40) & 255)::int),
    1, ((unix_ts_ms >> 32) & 255)::int),
    2, ((unix_ts_ms >> 24) & 255)::int),
    3, ((unix_ts_ms >> 16) & 255)::int),
    4, ((unix_ts_ms >> 8) & 255)::int),
    5, (unix_ts_ms & 255)::int);
  -- version 7 in the high nibble of byte 6; variant 10 in the high bits of byte 8
  uuid_bytes := set_byte(uuid_bytes, 6, ((get_byte(uuid_bytes,6) & 15) | 112));
  uuid_bytes := set_byte(uuid_bytes, 8, ((get_byte(uuid_bytes,8) & 63) | 128));
  RETURN encode(uuid_bytes, 'hex')::uuid;
END $$ LANGUAGE plpgsql VOLATILE;

-- updated_at maintenance trigger (B-R7). System-set; ignores any caller-supplied updated_at.
CREATE OR REPLACE FUNCTION touch_updated_at() RETURNS trigger AS $$
BEGIN NEW.updated_at := now(); RETURN NEW; END $$ LANGUAGE plpgsql;

-- The one RLS predicate, factored so every policy reads identically (B-R4/B-R5).
-- Returns the request tenant; raises if unset, so a missing SET LOCAL fails CLOSED (not open).
CREATE OR REPLACE FUNCTION app_tenant() RETURNS uuid AS $$
  SELECT NULLIF(current_setting('app.tenant_id', true), '')::uuid;
$$ LANGUAGE sql STABLE;

-- IMMUTABLE wrapper around unaccent() (which ships STABLE). Postgres requires an IMMUTABLE
-- expression for a GENERATED ALWAYS ... STORED column; the §6.1 search_tsv columns (Appendix B)
-- fold diacritics via unaccent, so this wrapper is the minimal adaptation that preserves the
-- diacritic-folded FTS semantics while satisfying the generated-column immutability constraint.
CREATE OR REPLACE FUNCTION immutable_unaccent(text) RETURNS text AS $$
  SELECT unaccent('unaccent', $1);
$$ LANGUAGE sql IMMUTABLE STRICT;

-- Reusable enums.
CREATE TYPE idea_kind        AS ENUM ('text','voice','clip');
CREATE TYPE ingest_state     AS ENUM ('pending','ingesting','ingested','skipped_redundant','failed');
CREATE TYPE wiki_page_type   AS ENUM ('entity','concept','source','index','log','contradiction','schema','comparison','overview');
CREATE TYPE msg_role         AS ENUM ('system','user','assistant','tool');
CREATE TYPE cred_status      AS ENUM ('ok','exhausted','dead');
CREATE TYPE job_state        AS ENUM ('pending','leased','done','failed','skipped','canceled');
CREATE TYPE job_kind         AS ENUM ('ingest','lint','dream','transcribe','sync_materialize','export');
CREATE TYPE approval_status  AS ENUM ('pending','approved','rejected','expired');
CREATE TYPE contra_status    AS ENUM ('detected','review_ok','resolved','pending_fix','suppressed');

-- ============================ §4.0 Roles & grants (M0) ============================
-- The application role. NOT superuser, NOT BYPASSRLS (B-R5). All app traffic uses it.
-- A migration role that owns DDL (separate so the app role can't ALTER tables at runtime).
-- Appendix B §4.0 verbatim:
--   CREATE ROLE qcue_app LOGIN PASSWORD :'app_pw' NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
--   CREATE ROLE qcue_migrator LOGIN PASSWORD :'mig_pw' NOSUPERUSER NOBYPASSRLS;
--   GRANT USAGE ON SCHEMA public TO qcue_app;
--   ALTER DEFAULT PRIVILEGES FOR ROLE qcue_migrator IN SCHEMA public
--     GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO qcue_app;
-- Permission-tolerant wrapper (the ONLY adaptation): create the roles + grants only when the
-- executing role may create roles; otherwise leave them to S3's privileged migrator.
DO $$
BEGIN
  IF (SELECT rolcreaterole FROM pg_roles WHERE rolname = current_user) THEN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'qcue_app') THEN
      CREATE ROLE qcue_app LOGIN PASSWORD 'qcue_app_dev' NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'qcue_migrator') THEN
      CREATE ROLE qcue_migrator LOGIN PASSWORD 'qcue_mig_dev' NOSUPERUSER NOBYPASSRLS;
    END IF;
    GRANT USAGE ON SCHEMA public TO qcue_app;
    ALTER DEFAULT PRIVILEGES FOR ROLE qcue_migrator IN SCHEMA public
      GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO qcue_app;
  END IF;
END $$;

-- Reusable helper for the per-table `GRANT ... TO qcue_app` lines that the RLS(t) expansion emits
-- (Appendix B §4 RLS block). The grant is conditional on the role existing so the migrations run
-- in environments where the app role has not been provisioned (e.g. the sqlx test fixture).
CREATE OR REPLACE FUNCTION _grant_app(tbl regclass) RETURNS void AS $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'qcue_app') THEN
    EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON %s TO qcue_app', tbl);
  END IF;
END $$ LANGUAGE plpgsql;
