# Open-Source Publishing Model

QCue is developed in a **private source-of-truth repository** and published to a separate **clean
public mirror**. This document explains the model, why it exists, and the exact, safe workflow for
producing and updating the public mirror.

## The two repositories

- **Private repository — source of truth.** Full development history, internal references, ops docs,
  experiments, and production/internal notes. It stays private. Its history is **never** rewritten and
  **never** published.
- **Public mirror (`../qcue-public`) — sanitized distribution.** A separate Git repository with its
  own **clean** history, containing only explicitly allowlisted, inspected, public-safe files plus
  AGPL-3.0 governance.

```
private qcue repo   →  source of truth, full history, private workspace (never published)
../qcue-public      →  AGPL-3.0 public mirror, clean history, public distribution
```

## Why the private history is not published

Git history is a **permanent exposure surface**. Once pushed publicly it cannot be reliably recalled.
A long private history may contain old secrets, internal ops docs, deployment topology, third-party
material that cannot be relicensed, experimental code, large artifacts, and sensitive commit
messages. **Deleting a file from the current tree does not remove it from history** — the blob and the
commit message remain. So instead of trying to scrub years of history, QCue ships a curated, audited
snapshot and develops openly from there.

## Why whitelist export (not blacklist)

A blacklist ("export everything except X") fails open: anything you forget to exclude leaks. A
**whitelist** ("export only these inspected paths") fails closed: anything not explicitly listed is
never published. QCue uses whitelist export as the primary safety model, with secret/risky-pattern
scanning as defense-in-depth on top.

## Why AGPL-3.0

QCue can be run as a network service. AGPL-3.0 closes the "SaaS loophole": anyone who offers a
modified QCue to users over a network must also make their modified source available. This keeps the
ecosystem open while QCue remains genuinely free software. See [`../LICENSE`](../LICENSE).

## What is exported

Only the allowlist in `scripts/public-export-allowlist.txt` (in the private repo). At time of writing
that is:

- Open-source governance files (from `oss/`): `LICENSE`, `README.md`, `NOTICE`, `SECURITY.md`,
  `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `THIRD_PARTY_NOTICES.md`, `TRADEMARKS.md`,
  `DEVELOPMENT_HISTORY.md`, `CHANGELOG.md`.
- Curated public docs (`docs/public/` → `docs/`).
- First-party source: `qcue-rs/`, `qcue_app/`, `design-system/` — **tracked files only** (build
  artifacts and untracked files are never copied).

## What is never exported

The private history (`.git`), `docs/references/`, internal/ops/runbooks/infra/deploy/production
folders, secrets/credentials, experiments, private prompts and notes, app-store / play-console /
provisioning material, infrastructure config, databases/dumps/backups/logs, archives, and any file
matching a risky secret/credential pattern. (See the export and check scripts for the full lists.)

## Tooling

From the private repository:

```sh
make public-audit-private   # scan the PRIVATE repo (tree + history) for secrets — read-only
make public-init            # create/verify the ../qcue-public mirror (git init only)
make public-export          # copy allowlisted, inspected files into ../qcue-public
make public-check           # safety-scan the public mirror (fails closed)
make public-status          # show the mirror's git status
make public-dry-run         # PRINT the manual commit/remote/push commands (does not run them)
```

Equivalently, the scripts under `scripts/` can be run directly.

## Initializing the mirror

```sh
make public-init            # runs scripts/init-public-mirror.sh ../qcue-public
```

This creates `../qcue-public` (outside and separate from the private repo) and runs `git init -b
main`. It does **not** add a remote, copy any `.git`, or push.

## Committing the public mirror (manual, deliberate)

Committing and pushing are **never** automated. After `make public-export` and a clean `make
public-check`, do it by hand **inside the mirror**:

```sh
cd ../qcue-public
git status
git add -A
git commit -m "Initial public release"
git remote add origin <PUBLIC_REPO_URL>   # add the remote ONLY inside ../qcue-public
git push -u origin main                   # push ONLY from ../qcue-public
```

The public remote is added **only inside `../qcue-public`** — never to the private repo.

## Receiving public contributions back into the private repo

Public pull requests land in `../qcue-public`. They are **not** merged wholesale into the private
repo. Instead, review the PR, confirm it touches only public-safe paths, then cherry-pick / apply the
specific change into the private source of truth, run tests, re-export, and re-check. See
`scripts/apply-public-patch-notes.md`.

## Forbidden commands

These are never run as part of publishing:

- `git push --mirror` (would push everything, including private history)
- `git filter-repo` on the private repo (history rewrite)
- changing the private repository's visibility
- pushing private branches to the public remote
- adding the public remote to the **private** repo
- exporting `docs/references/`
- exporting ops / runbooks / deploy / infra / production paths

## Secret handling

- A secret that has **ever** been committed must be treated as compromised: **rotate/revoke it**, do
  not rely on deletion.
- The public mirror has a clean history, but the **private** repository's history may still contain
  old secrets — clean public history does not retroactively clean private history. Audit and rotate as
  needed.
- Never print, paste, or log real secret values.

## Release checklist

1. `make public-audit-private` → review findings; rotate/revoke anything flagged in history.
2. `make public-export` → regenerate the mirror from the allowlist.
3. `make public-check` → must be **PASS** (no private dirs, no risky files, no oversized blobs, no
   secret patterns).
4. Review `THIRD_PARTY_NOTICES.md` and `CHANGELOG.md`; update as needed.
5. Manually `git add`/`commit` inside `../qcue-public`.
6. Add the remote (first time only) and `git push` **from `../qcue-public`**.
7. Confirm the published tree on the public host matches the local mirror.
