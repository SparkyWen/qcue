<!--
PR title must follow Conventional Commits, e.g. "fix(router): rotate on 429".
Allowed types: feat, fix, docs, refactor, perf, test, build, ci, chore, release, revert.
-->

## Summary

<!-- What does this change do, and why? -->

## Related issue

<!-- e.g. Closes #123. For anything non-trivial, please open an issue first. -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Documentation
- [ ] Refactor / chore / CI
- [ ] Other:

## Checklist

- [ ] PR title follows Conventional Commits.
- [ ] Touched **Rust** (`qcue-rs/`)? Ran `cargo clippy --all-targets -- -D warnings`, `cargo run -p xtask`, and `cargo test` (or `cargo test --lib` for the keyless/DB-free subset).
- [ ] Touched **Flutter** (`qcue_app/`)? Ran `flutter analyze` and `flutter test`.
- [ ] Changed wire/protocol types? Regenerated codegen with `cargo run -p app-server-protocol --bin export-schema`.
- [ ] Added/updated tests for new behavior where the area is testable.
- [ ] **No secrets** (keys, tokens, credentials, production config) in code, commits, or screenshots.

## Notes for reviewers

<!-- Anything that needs special attention, screenshots, trade-offs, follow-ups. -->
