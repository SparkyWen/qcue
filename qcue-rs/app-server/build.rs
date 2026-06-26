//! QCue — embed build provenance (git SHA + dirty flag + build time) into the `app-server` binary so
//! `GET /version` can answer "is prod running the code I merged?" WITHOUT SSH-ing in and grepping the
//! binary. This is the structural fix for the 2026-06-17 stale-binary incident
//! (docs/postmortems/2026-06-17-stale-binary-incident.md): the deploy script asserts the live
//! `/version` SHA equals the SHA it just built, closing the merge≠deploy gap automatically.
//!
//! Resolution order for each value (so CI / the deploy script are authoritative, local dev still works):
//!   1. an explicit `QCUE_BUILD_{SHA,DIRTY,TIME}` env var (set by `scripts/deploy-prod.sh`), else
//!   2. `git` in the source tree, else
//!   3. `"unknown"` (e.g. building from a tarball with no git) — the /version test rejects this for SHA.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn main() {
    let sha = env_nonempty("QCUE_BUILD_SHA")
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());

    // Dirty = uncommitted changes in the work tree at build time. A dirty prod binary is a red flag.
    let dirty = env_nonempty("QCUE_BUILD_DIRTY").unwrap_or_else(|| {
        match git(&["status", "--porcelain"]) {
            Some(s) if !s.is_empty() => "true".to_string(),
            Some(_) => "false".to_string(),
            None => "unknown".to_string(),
        }
    });

    // RFC-3339 UTC build time. `date` avoids pulling a chrono build-dep into a leaf build script.
    let built_at = env_nonempty("QCUE_BUILD_TIME")
        .or_else(|| git(&["log", "-1", "--format=%cI"])) // commit time is a stable, reproducible fallback
        .or_else(|| {
            Command::new("date")
                .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();

    println!("cargo:rustc-env=QCUE_GIT_SHA={sha}");
    println!("cargo:rustc-env=QCUE_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=QCUE_BUILD_TIME={built_at}");

    // Rebuild this script's output when the deploy env or the checked-out commit changes, so the
    // embedded SHA never goes stale relative to what's actually being compiled.
    println!("cargo:rerun-if-env-changed=QCUE_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=QCUE_BUILD_DIRTY");
    println!("cargo:rerun-if-env-changed=QCUE_BUILD_TIME");
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(ref_path) = git(&["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&["rev-parse", "--git-path", &r])) {
        println!("cargo:rerun-if-changed={ref_path}");
    }
}
