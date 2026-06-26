# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once public releases begin.

## [Unreleased]

## [1.1.0] - 2026-06-26

First public **AGPL-3.0 source release** of QCue, and its first public Android APK. The public
release line starts at 1.1.0; earlier 1.0.x builds were private/internal and pre-public.

### Added

- Public AGPL-3.0 source release: the LLM harness core, the idea engine, and the multi-tenant Axum
  backend (`qcue-rs/`), plus the offline-first Flutter app (`qcue_app/`).
- First public Android APK, built from the matching public `v1.1.0` source tag and upload-key signed
  (internal `versionName` `1.1.0`). Provided for direct sideload install.
- Open-source governance: AGPL-3.0 `LICENSE`, `NOTICE`, third-party notices (including the Flutter
  engine license sidecar), `SECURITY.md`, contribution and code-of-conduct guides.
- Bilingual (English / 简体中文) README and curated public documentation.
- Clean, audited public repository history, independent of the private source-of-truth repo.

### Notes

- Google Play distribution is not yet available; iOS distribution continues via the App Store / TestFlight.
- The public mirror is generated only through the whitelist export workflow — no private history, ops
  docs, internal references, or secrets are included.

<!--
Future entries go above this comment, newest first, e.g.:

## [0.1.0] - YYYY-MM-DD
### Added
- ...
### Changed
- ...
### Fixed
- ...
-->
