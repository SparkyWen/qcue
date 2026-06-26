-- QCue LOC-R1 (spec 2026-06-18 §Part E) — optional precise capture location. Nullable, additive; a
-- capture without location (toggle off / permission denied / no fix) simply leaves these NULL. Fine GPS
-- is opt-in (off by default) and captured at action-time in the app's unified capture funnel. No
-- spatial index / pgvector (M6 rule) — location is stored context, not a query dimension yet.
ALTER TABLE ideas ADD COLUMN lat            DOUBLE PRECISION;  -- WGS84 latitude  (NULL = no location)
ALTER TABLE ideas ADD COLUMN lng            DOUBLE PRECISION;  -- WGS84 longitude (NULL = no location)
ALTER TABLE ideas ADD COLUMN loc_accuracy_m REAL;             -- horizontal accuracy in metres (optional)
