# Security Policy

## Reporting a vulnerability

**Do not open a public GitHub issue for security vulnerabilities or for anything involving secrets.**

Report privately to **helios@sinox.ai**. Please include:

- a description of the issue and its impact,
- steps to reproduce (a minimal proof of concept if possible),
- affected component(s) and version/commit, and
- any suggested remediation.

You will receive an acknowledgement, and we ask that you give us a reasonable window to investigate
and ship a fix before any public disclosure (coordinated / responsible disclosure). Please act in
good faith: avoid privacy violations, data destruction, service disruption, and accessing or
modifying data that is not yours while researching.

## Do not include secrets in a report

If your report would require pasting a real API key, token, credential, private key, or production
configuration value, **redact it**. We never need the real secret value to reproduce or fix an
issue. If you believe a secret has been exposed, the correct response is to **rotate/revoke it
immediately** — treat any secret that has ever appeared in a repository, log, or screenshot as
compromised.

## BYOK security note

QCue is a bring-your-own-key system. Provider API keys belong to **you** and must never be committed
to source control, pasted into issues or pull requests, or embedded in screenshots, recordings, or
logs. The codebase is designed so that credentials are encrypted at rest and redacted at logging and
persistence boundaries; contributions must preserve those invariants.

## What this public repository excludes

For safety, this public repository **excludes** private production configuration, deployment
runbooks, infrastructure topology, and internal operational documentation. A security report should
not assume the presence of any such material here; describe the concern against the public source.

## Supported versions

| Version      | Supported          |
| ------------ | ------------------ |
| `main`       | ✅ (active)         |
| Older tags   | TODO — to be defined as releases are cut |

The supported-versions matrix will be formalized once the first public releases are tagged.
