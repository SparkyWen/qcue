# Development History

QCue had substantial development **before** this public release. That work is real, but it lived in a
private repository, and its full Git history is intentionally **not** published here.

## Why a clean public history

Git history is permanent and public forever once pushed. A long private history can carry things that
should never be distributed:

- secrets that once existed in a commit (API keys, tokens, credentials, private keys);
- internal operations notes, deployment topology, and production configuration;
- references to private accounts, infrastructure, vendors, or customers in commit messages;
- third-party material that cannot be relicensed under AGPL-3.0;
- experimental code, throwaway prompts, and test data;
- large artifacts that make a repository slow and unpleasant to clone.

Deleting a file from the current tree does **not** remove it from history. Rather than rewrite a long
history and hope nothing was missed, QCue publishes a **curated, audited snapshot** and develops
openly from there. This is a security and maintainability decision — not an attempt to hide the
project's maturity.

## What remains private

- The full private Git history and commit messages.
- Internal references, research notes, and experiments.
- Operations docs, deployment runbooks, and infrastructure/production configuration.
- Anything that is not explicitly on the public export allowlist.

## What is public

- The first-party application source: the Rust workspace (`qcue-rs/`), the Flutter app
  (`qcue_app/`), and the design system (`design-system/`).
- Curated public documentation under `docs/`.
- Open-source governance: license, security policy, contributing guide, code of conduct, and notices.

## How future public development is tracked

Public trust in this project should come from **what can be inspected today**, not from old private
commits:

- audited, readable source code;
- public documentation and design notes;
- the security policy and responsible-disclosure process;
- issue and pull-request discussion in the open;
- reproducible builds and tagged releases over time;
- all **future** commits, which happen openly on this repository's clean history.

## Milestones

Public milestones will be recorded here and in [`CHANGELOG.md`](./CHANGELOG.md) as releases are cut.
To avoid misrepresenting private history, specific historical dates are intentionally omitted.

- **TODO:** Initial public release — date and contents to be filled in when the first public tag is
  published.
- **TODO:** Subsequent public milestones.
