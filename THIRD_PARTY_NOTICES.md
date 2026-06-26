# Third-Party Notices

QCue depends on third-party open-source components. Each such component remains under its **own**
license; this file tracks notices and attributions for them. The QCue source itself is licensed
under AGPL-3.0 (see [`LICENSE`](./LICENSE)).

> **Status:** This is a working document. A full, automated dependency-license audit has **not** yet
> been completed and recorded here. Sections below are marked `TODO` until that audit is run. No
> claim of complete third-party license review or compatibility is made beyond what is explicitly
> listed.

## Vendored / copied third-party source code

A review of the publicly exported trees (`qcue-rs/`, `qcue_app/`, `design-system/`) did not identify
copied or vendored third-party **source code** committed into them; dependencies are consumed through
their respective package managers rather than copied in. If vendored code is ever added, it must be
recorded here with its license and upstream attribution.

- Status: none identified in exported trees. (TODO: re-confirm on each public export.)

## Rust dependencies (`qcue-rs/`, via Cargo)

Crate dependencies are declared in `Cargo.toml` / `Cargo.lock` and fetched from crates.io under their
own licenses (commonly MIT / Apache-2.0 and others).

- TODO: generate the full dependency-license list (e.g. `cargo deny check licenses`,
  `cargo about generate`, or `cargo license`) and record it here.

## Flutter / mobile dependencies (`qcue_app/`, via pub)

Dart/Flutter package dependencies are declared in `pubspec.yaml` / `pubspec.lock` and fetched from
pub.dev under their own licenses.

- TODO: generate the Flutter dependency-license list (e.g. via the app's licenses page /
  `flutter pub deps`) and record it here.

## Node / web dependencies

- TODO: if/when web or Node tooling is added to the public tree, record its dependency licenses here.

## License texts

When this audit is completed, full upstream license texts for components that require reproduction
will be included or linked from this file.
