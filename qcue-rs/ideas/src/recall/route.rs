// QCue B-R24/B-R25 — recall routing re-export. The `route_search`/`SearchMode` logic lives in the leaf
// `search-route` crate so `store` (the SQL executor) and `ideas` (the tool) share it without a sibling
// import. `qcue_ideas::recall::route::{route_search, SearchMode}` stays a stable path for callers/tests.
pub use search_route::{is_cjk, route_search, SearchMode};
