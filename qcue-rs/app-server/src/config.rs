// QCue S3-R14/S3-R68/S3-R70/S3-R72 — config with refuse-to-boot validation + dev isolation.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("JWT secret must be >=32 bytes and not a known dev default")]
    WeakSecret,
    #[error("QCUE_KMS_KEY, when set, must be >=32 bytes")]
    WeakKmsKey,
    #[error("DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("QCUE_DATA_ROOT is required")]
    MissingDataRoot,
}

#[derive(Clone, Debug)]
pub struct RawConfig {
    /// AU-R8 — path to the on-disk release manifest JSON the deploy step writes. Empty ⇒ the
    /// `/v1/app/release` endpoint degrades benignly (returns latest_build:0), never 5xx.
    pub release_manifest_path: String,
    /// AU-R11 — read-scoped GitHub token for proxying the private-repo release APK. None ⇒ the
    /// `/v1/app/apk/{build}` proxy returns 503 (no source configured). NEVER logged (S1-R38).
    pub github_token: Option<String>,
    /// Path to the on-disk `assetlinks.json` (Android App Links). Empty ⇒ `/.well-known/assetlinks.json`
    /// returns 404 (the App Links flow isn't live until the operator drops the file in place).
    pub assetlinks_path: String,
    pub jwt_secret: String,
    pub database_url: String,
    pub auth_database_url: String,
    pub redis_url: String,
    pub redis_prefix: String,
    pub data_root: String,
    pub bind_addr: String,
    pub bind_port: u16,
    pub app_origins: Vec<String>,
    pub trusted_proxy: String,
    pub ingest_enabled: bool,
    pub lint_enabled: bool,
    pub dream_enabled: bool,
    pub sync_enabled: bool,
    pub key_proxy_enabled: bool,
    /// Access-token lifetime in seconds (AUTH-D6). Default 3600; the 30-day session
    /// comes from reliable refresh, not a long access token.
    pub access_ttl_secs: i64,
    /// Google OAuth client IDs accepted as the `aud` of a Google id_token at POST /v1/auth/social
    /// (NG-R7): the web client id (Android tokens) + the iOS client id (iOS tokens).
    pub google_oauth_audiences: Vec<String>,
    /// Apple bundle id(s) accepted as the `aud` of an Apple identity token at POST /v1/auth/social
    /// (SIWA-R1). Native iOS flow → the app bundle id `cn.qcue.app` (NOT a Services ID). Empty =
    /// Apple sign-in disabled.
    pub apple_oauth_audiences: Vec<String>,
    /// Master key (>=32 bytes) for the BYOK vault's real KMS (`EnvKms`), from `QCUE_KMS_KEY`. When
    /// absent the server falls back to the INSECURE dev `StubKms` (public-constant KEK) and logs a loud
    /// warning — production MUST set this so stored API keys are actually encrypted at rest (S1-R38).
    pub kms_key: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// AU-R8 — path to the on-disk release manifest JSON (`QCUE_RELEASE_MANIFEST`). Empty ⇒ degrade benignly.
    pub release_manifest_path: String,
    /// AU-R11 — read-scoped GitHub token for the private-repo APK proxy. None ⇒ proxy 503. NEVER logged.
    pub github_token: Option<String>,
    /// Path to the on-disk `assetlinks.json` (`QCUE_ASSETLINKS_PATH`). Empty ⇒ the well-known route 404s.
    pub assetlinks_path: String,
    pub jwt_secret: Vec<u8>,
    pub database_url: String,
    pub auth_database_url: String,
    pub redis_url: String,
    pub redis_prefix: String,
    pub data_root: String,
    pub bind_addr: String,
    pub bind_port: u16,
    pub app_origins: Vec<String>,
    pub trusted_proxy: String,
    pub ingest_enabled: bool,
    pub lint_enabled: bool,
    pub dream_enabled: bool,
    pub sync_enabled: bool,
    pub key_proxy_enabled: bool,
    pub access_ttl_secs: i64,
    pub google_oauth_audiences: Vec<String>,
    pub apple_oauth_audiences: Vec<String>,
    pub kms_key: Option<Vec<u8>>,
}

const KNOWN_DEV_DEFAULTS: &[&str] = &["changeme", "secret", "dev", "test", "password"];

impl Config {
    pub fn validate(r: RawConfig) -> Result<Config, ConfigError> {
        if r.jwt_secret.len() < 32
            || KNOWN_DEV_DEFAULTS.iter().any(|d| r.jwt_secret.eq_ignore_ascii_case(d))
        {
            return Err(ConfigError::WeakSecret);
        }
        if r.database_url.is_empty() {
            return Err(ConfigError::MissingDatabaseUrl);
        }
        if r.data_root.is_empty() {
            return Err(ConfigError::MissingDataRoot);
        }
        if let Some(k) = &r.kms_key
            && k.len() < 32
        {
            return Err(ConfigError::WeakKmsKey);
        }
        Ok(Config {
            release_manifest_path: r.release_manifest_path,
            github_token: r.github_token,
            assetlinks_path: r.assetlinks_path,
            jwt_secret: r.jwt_secret.into_bytes(),
            database_url: r.database_url,
            auth_database_url: r.auth_database_url,
            redis_url: r.redis_url,
            redis_prefix: r.redis_prefix,
            data_root: r.data_root,
            bind_addr: r.bind_addr,
            bind_port: r.bind_port,
            app_origins: r.app_origins,
            trusted_proxy: r.trusted_proxy,
            ingest_enabled: r.ingest_enabled,
            lint_enabled: r.lint_enabled,
            dream_enabled: r.dream_enabled,
            sync_enabled: r.sync_enabled,
            key_proxy_enabled: r.key_proxy_enabled,
            access_ttl_secs: r.access_ttl_secs,
            google_oauth_audiences: r.google_oauth_audiences,
            apple_oauth_audiences: r.apple_oauth_audiences,
            kms_key: r.kms_key,
        })
    }

    pub fn from_env() -> Result<Config, ConfigError> {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let flag = |k: &str| std::env::var(k).map(|v| v == "true").unwrap_or(false);
        Config::validate(RawConfig {
            release_manifest_path: env("QCUE_RELEASE_MANIFEST"),
            github_token: {
                let t = env("QCUE_GITHUB_TOKEN");
                if t.is_empty() { None } else { Some(t) }
            },
            assetlinks_path: env("QCUE_ASSETLINKS_PATH"),
            jwt_secret: env("QCUE_JWT_SECRET"),
            database_url: env("DATABASE_URL"),
            auth_database_url: {
                let a = env("DATABASE_AUTH_URL");
                if a.is_empty() { env("DATABASE_URL") } else { a }
            },
            redis_url: env("REDIS_URL"),
            redis_prefix: env("QCUE_REDIS_PREFIX"),
            data_root: env("QCUE_DATA_ROOT"),
            bind_addr: {
                let a = env("QCUE_BIND_ADDR");
                if a.is_empty() { "127.0.0.1".into() } else { a }
            },
            bind_port: env("QCUE_BIND_PORT").parse().unwrap_or(9200),
            app_origins: env("QCUE_APP_ORIGINS")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            trusted_proxy: env("QCUE_TRUSTED_PROXY"),
            ingest_enabled: flag("INGEST_WORKERS_ENABLED"),
            lint_enabled: flag("LINT_WORKERS_ENABLED"),
            dream_enabled: flag("DREAM_WORKERS_ENABLED"),
            sync_enabled: flag("SYNC_WORKERS_ENABLED"),
            key_proxy_enabled: flag("QCUE_KEY_PROXY_ENABLED"),
            access_ttl_secs: std::env::var("QCUE_ACCESS_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&v: &i64| v > 0)
                .unwrap_or(3600),
            google_oauth_audiences: env("GOOGLE_OAUTH_AUDIENCES")
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            apple_oauth_audiences: env("APPLE_OAUTH_AUDIENCES")
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            kms_key: {
                let k = env("QCUE_KMS_KEY");
                if k.is_empty() { None } else { Some(k.into_bytes()) }
            },
        })
    }

    pub fn test_raw() -> RawConfig {
        RawConfig {
            release_manifest_path: String::new(),
            github_token: None,
            assetlinks_path: String::new(),
            jwt_secret: "dev-only-secret-please-change-32bytes!!".into(),
            database_url: "postgres://x".into(),
            auth_database_url: "postgres://x".into(),
            redis_url: "redis://localhost/1".into(),
            redis_prefix: "devtest".into(),
            data_root: "/tmp/qcue-test-data".into(),
            bind_addr: "127.0.0.1".into(),
            bind_port: 9201,
            app_origins: vec!["http://localhost:3000".into()],
            trusted_proxy: "127.0.0.1".into(),
            ingest_enabled: false,
            lint_enabled: false,
            dream_enabled: false,
            sync_enabled: false,
            key_proxy_enabled: false,
            access_ttl_secs: 3600,
            google_oauth_audiences: vec![],
            apple_oauth_audiences: vec![],
            kms_key: None,
        }
    }
}

#[cfg(test)]
mod release_cfg_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn defaults_are_benign_when_release_env_absent() {
        let cfg = Config::validate(Config::test_raw()).unwrap();
        assert_eq!(cfg.release_manifest_path, "");
        assert!(cfg.github_token.is_none());
    }
}
