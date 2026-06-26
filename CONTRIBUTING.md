# Contributing to QCue

Thank you for your interest in contributing. This document explains how to propose changes and the
rules that keep the public repository safe and legally clean.

## Licensing of contributions

Unless explicitly stated otherwise, contributions are submitted under the project's license,
**AGPL-3.0** (see [`LICENSE`](./LICENSE)). By opening a pull request, you affirm that you have the
right to contribute the code and that it may be distributed under AGPL-3.0.

## Never include secrets or private material

Contributions — including code, commits, commit messages, issues, pull requests, screenshots,
recordings, and logs — **must not** contain:

- API keys, tokens, credentials, private keys, or passwords;
- production configuration, deployment runbooks, infrastructure topology, or internal operational
  notes;
- private third-party material you do not have the right to relicense under AGPL-3.0;
- personal data of yourself or others.

If you ever expose a secret, **rotate/revoke it immediately** and notify a maintainer privately
(see [`SECURITY.md`](./SECURITY.md)). Deleting a file does not remove a secret from history.

## Third-party code

If you add or vendor third-party code, ensure its license is compatible with AGPL-3.0, preserve its
notices, and record it in [`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md). When in doubt, ask
before adding it.

## Workflow

1. **Discuss first for anything large.** For non-trivial or architectural changes, open an issue and
   ask maintainers before investing significant effort. Small fixes can go straight to a PR.
2. **Fork and branch.** Work on a topic branch in your fork; keep one logical change per PR.
3. **Match existing patterns.** Follow the conventions already present in the area you're touching.
4. **Run the checks that apply** to what you changed:
   - Rust (`qcue-rs/`): `cargo test`, `cargo clippy --all-targets -- -D warnings`
   - Flutter (`qcue_app/`): `flutter test`, `flutter analyze`
   - (Some integration tests require local Postgres/Redis; see [`docs/architecture.md`](./docs/architecture.md).)
5. **Write tests** for new behavior where the area is testable.
6. **Open the PR** with a clear description of the problem and the approach. Link the issue if one
   exists.

## Code of conduct

All participation is governed by the [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md).

## How accepted contributions are integrated

This public repository is a curated mirror. The project is developed primarily in a private
source-of-truth repository, and accepted public contributions are reviewed and applied back into it
through a sanitized workflow before being re-published here. Practically, this means your merged
change may be re-exported (rather than fast-forwarded) on the next public sync — your authorship and
the change itself are preserved; the mechanics are described in the project's publishing
documentation.
