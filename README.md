# QCue

QCue is an open-source, **BYOK** (bring-your-own-key) knowledge, memory, and semantic-recall
system — a capture-first "second brain." Fleeting ideas (typed or spoken) land in a daily feed, a
BYOK large-language model distills them into a linked Markdown wiki (`[[wikilinks]]`, `index.md`,
`log.md`), and a nightly consolidation pass keeps the knowledge base coherent. Everything runs
against **your own** model provider key — QCue never ships or brokers credentials.

The project is three real code areas:

- **`qcue-rs/`** — a Rust workspace: a provider-agnostic LLM **harness**, the **idea engine**
  (capture → distill → recall → consolidate), and a multi-tenant **Axum backend**.
- **`qcue_app/`** — a Flutter app (Android + iOS), offline-first, with a stub mode that runs the
  whole product keyless and networkless.
- **`design-system/`** — the shared visual language and design tokens.

---

## A note on this repository's history

> This repository starts with a **clean public history** for security, third-party license
> hygiene, and internal reference protection. The original private development history is preserved
> separately.

This is a deliberate engineering decision, **not** an attempt to hide the project's maturity. Git
history is a permanent exposure surface: it can carry old secrets, internal operations notes,
deployment topology, third-party material that cannot be relicensed, and large artifacts that make a
repository slow to clone. Rather than rewrite a long private history and hope nothing was missed,
QCue publishes a curated, audited snapshot and tracks all future development openly from here.

See [`DEVELOPMENT_HISTORY.md`](./DEVELOPMENT_HISTORY.md) for the full rationale and what is / isn't
included.

---

## Quickstart

> The commands below run the product locally with **no API key and no network** via the built-in
> stub. Live-backend and provider-key setup are documented in
> [`docs/architecture.md`](./docs/architecture.md). Some end-to-end and deployment steps are marked
> `TODO` where they depend on environment specifics.

### App (Flutter)

```sh
cd qcue_app
flutter pub get
flutter test                                      # unit + widget + architecture tests
flutter analyze
flutter run --dart-define=QCUE_STUB=true          # offline/demo, seeded fixtures, keyless
```

### Backend (Rust)

```sh
cd qcue-rs
cargo test --lib                 # fast unit tests, no database required
cargo clippy --all-targets -- -D warnings
# Full integration suite (Postgres 16 + Redis 7) and running the server: see docs/architecture.md
# TODO: document the minimal local Postgres/Redis bootstrap for external contributors
```

---

## License

QCue is licensed under the **GNU Affero General Public License, version 3 (AGPL-3.0)**. See
[`LICENSE`](./LICENSE). Because QCue can be operated as a network service, AGPL-3.0 ensures that
modifications offered to users over a network are also made available as source. See also
[`NOTICE`](./NOTICE) and [`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md).

## Security

Please report vulnerabilities **privately** — do not open public issues for security matters. See
[`SECURITY.md`](./SECURITY.md). BYOK reminder: never commit provider API keys, tokens, credentials,
private keys, or production configuration.

## Contributing

Contributions are welcome. Start with [`CONTRIBUTING.md`](./CONTRIBUTING.md) and the
[`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md).

## Trademark

The QCue name, logo, and brand assets are reserved. The AGPL-3.0 grant covers source code, not brand
usage. See [`TRADEMARKS.md`](./TRADEMARKS.md).

## Development history

Why this repository begins with a clean history, what remains private, and how future development is
tracked openly: [`DEVELOPMENT_HISTORY.md`](./DEVELOPMENT_HISTORY.md).

## Changelog

Notable changes are recorded in [`CHANGELOG.md`](./CHANGELOG.md).
