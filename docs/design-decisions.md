# Design Decisions (Public ADRs)

Lightweight architecture decision records for choices that are visible in the public repository. Each
records the decision and why, without exposing private infrastructure or internal detail. More
records will be added as the public project evolves.

---

## ADR-0001: Private source-of-truth + clean public mirror

**Decision.** Develop QCue in a private source-of-truth repository and publish a separate public
mirror with a **clean** initial history, rather than making the existing private repository public or
rewriting its history.

**Why.**

- **Safety.** Git history is a permanent exposure surface. A clean public history cannot leak old
  secrets, internal notes, or sensitive commit messages, because none of that history is present.
- **License hygiene.** Publishing only inspected, first-party, allowlisted files avoids redistributing
  third-party material that cannot be relicensed.
- **Third-party code hygiene.** The private workspace includes a large read-only study corpus and
  other material that must never be published; a whitelist export structurally prevents it from
  leaking.
- **Internal reference protection.** Ops docs, experiments, and research notes stay private without
  having to be individually scrubbed.
- **Professional presentation.** A small, clean, fast-to-clone repository is a better contributor
  experience than a multi-gigabyte history full of artifacts.

**Trade-off.** Public contributors do not see the original commit history. This is mitigated by
publishing audited source, public docs, and tracking all **future** development openly. See
[`open-source-publishing.md`](./open-source-publishing.md) and
[`DEVELOPMENT_HISTORY.md`](DEVELOPMENT_HISTORY.md).

---

## ADR-0002: AGPL-3.0 license

**Decision.** License QCue under AGPL-3.0.

**Why.** QCue can be run as a network service. AGPL-3.0 closes the "SaaS loophole" so that network
users of a modified QCue can obtain the corresponding source. This keeps the project genuinely open
while discouraging closed-source hosted derivatives.

**Trade-off.** Some organizations restrict AGPL usage, which can reduce certain corporate
contributions. This was accepted in favor of stronger copyleft protection for a hosted product.

---

## ADR-0003: Whitelist export, not blacklist

**Decision.** The public mirror is produced by an explicit allowlist of inspected paths, with
secret/risky-pattern scanning layered on top.

**Why.** A blacklist fails open (forgetting to exclude something leaks it); a whitelist fails closed
(anything not listed is never published). Failing closed is the correct default for a
publish-to-the-world operation.

---

## ADR-0004: Provider-agnostic harness (data, not code)

**Decision.** Adding an LLM provider is expressed as declarative data (a profile, a few stateless
hooks, wire-quirk table entries) consumed by a single turn loop that never branches on provider name.

**Why.** It keeps the core loop small and stable as providers are added, enables cross-provider
fallback without re-encoding, and makes provider behavior testable through a scripted stub.

---

## ADR-0005: Database row-level security for multi-tenancy

**Decision.** Enforce tenant isolation at the database layer (row-level security) rather than relying
on application-level filtering.

**Why.** It makes isolation a property of the data store instead of every query, so a missed filter
cannot silently leak across tenants.
