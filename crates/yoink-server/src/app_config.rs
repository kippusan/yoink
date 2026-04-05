use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;
use url::Url;

use crate::db::quality::Quality;

const DEFAULT_MUSIC_ROOT: &str = "./music";
const DEFAULT_DATABASE_URL: &str = "sqlite:./yoink.db?mode=rwc";
const DEFAULT_LOG_FORMAT: &str = "pretty";
const DEFAULT_SLSKD_BASE_URL: &str = "http://127.0.0.1:5030";
const DEFAULT_SLSKD_DOWNLOADS_DIR: &str = "./development/slskd-data/downloads";

#[derive(Debug, Error)]
pub(crate) enum AppConfigError {
    #[error(transparent)]
    Parse(#[from] envious::EnvDeserializationError),
    #[error("{0}")]
    Validation(String),
}

#[derive(Debug, Clone)]
pub(crate) struct AuthConfig {
    pub(crate) enabled: bool,
    pub(crate) session_secret: String,
    pub(crate) init_admin_username: Option<String>,
    pub(crate) init_admin_password: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAppConfig {
    /// Base URL for the Tidal hifi-api proxy.
    #[serde(default)]
    tidal_api_base_url: String,

    /// Whether the Tidal provider is enabled. Defaults to true.
    #[serde(default = "default_true")]
    tidal_enabled: bool,

    /// Whether the MusicBrainz metadata provider is enabled. Defaults to true.
    #[serde(default = "default_true")]
    musicbrainz_enabled: bool,

    /// Whether the Deezer metadata provider is enabled. Defaults to true.
    #[serde(default = "default_true")]
    deezer_enabled: bool,

    /// Whether the SoulSeek download source is enabled. Defaults to false.
    #[serde(default)]
    soulseek_enabled: bool,

    /// Base URL for the slskd REST API.
    #[serde(default = "default_slskd_base_url")]
    slskd_base_url: String,

    /// Optional slskd web username for JWT auth.
    #[serde(default)]
    slskd_username: String,

    /// Optional slskd web password for JWT auth.
    #[serde(default)]
    slskd_password: String,

    /// Local path to slskd downloads directory.
    #[serde(default = "default_slskd_downloads_dir")]
    slskd_downloads_dir: String,

    #[serde(default = "default_music_root")]
    music_root: String,

    #[serde(default = "default_quality")]
    default_quality: String,

    #[serde(default = "default_database_url")]
    database_url: String,

    #[serde(default = "default_log_format")]
    log_format: String,

    #[serde(default)]
    download_lyrics: bool,

    /// Maximum number of tracks to download in parallel per album job.
    #[serde(default = "default_download_max_parallel_tracks")]
    download_max_parallel_tracks: usize,

    #[serde(default)]
    auth_disabled: bool,

    #[serde(default)]
    auth_session_secret: String,

    #[serde(default)]
    auth_init_admin_username: String,

    #[serde(default)]
    auth_init_admin_password: String,
}

#[derive(Debug)]
pub(crate) struct AppConfig {
    pub(crate) tidal_api_base_url: Option<Url>,
    pub(crate) tidal_enabled: bool,
    pub(crate) musicbrainz_enabled: bool,
    pub(crate) deezer_enabled: bool,
    pub(crate) soulseek_enabled: bool,
    pub(crate) slskd_base_url: Option<Url>,
    pub(crate) slskd_username: String,
    pub(crate) slskd_password: String,
    pub(crate) slskd_downloads_dir: String,
    pub(crate) music_root: String,
    pub(crate) default_quality: Quality,
    pub(crate) database_url: String,
    pub(crate) log_format: String,
    pub(crate) download_lyrics: bool,
    pub(crate) download_max_parallel_tracks: usize,
    pub(crate) auth: AuthConfig,
}

impl AppConfig {
    pub(crate) fn from_env() -> Result<Self, AppConfigError> {
        let raw = config_parser().build_from_env::<RawAppConfig>()?;
        Self::from_raw(raw)
    }

    pub(crate) fn music_root_path(&self) -> PathBuf {
        PathBuf::from(&self.music_root)
    }

    fn from_raw(raw: RawAppConfig) -> Result<Self, AppConfigError> {
        let tidal_api_base_url = parse_optional_url(&raw.tidal_api_base_url, "TIDAL_API_BASE_URL")?;
        let slskd_base_url = Some(parse_url(
            &normalize_string(&raw.slskd_base_url, DEFAULT_SLSKD_BASE_URL),
            "SLSKD_BASE_URL",
        )?);
        let slskd_downloads_dir =
            normalize_string(&raw.slskd_downloads_dir, DEFAULT_SLSKD_DOWNLOADS_DIR);
        let slskd_username = normalize_string_opt(&raw.slskd_username).unwrap_or_default();
        let slskd_password = normalize_string_opt(&raw.slskd_password).unwrap_or_default();
        let music_root = normalize_string(&raw.music_root, DEFAULT_MUSIC_ROOT);
        let default_quality = parse_quality(&raw.default_quality);
        let database_url = normalize_string(&raw.database_url, DEFAULT_DATABASE_URL);
        let log_format = normalize_log_format(&raw.log_format);
        let download_max_parallel_tracks = raw.download_max_parallel_tracks.clamp(1, 16);
        let auth_session_secret =
            normalize_string_opt(&raw.auth_session_secret).unwrap_or_default();
        let init_admin_username = normalize_string_opt(&raw.auth_init_admin_username);
        let init_admin_password = normalize_string_opt(&raw.auth_init_admin_password);

        let auth = Self::build_auth_config(
            raw.auth_disabled,
            auth_session_secret,
            init_admin_username,
            init_admin_password,
        )?;

        Ok(Self {
            tidal_api_base_url,
            tidal_enabled: raw.tidal_enabled,
            musicbrainz_enabled: raw.musicbrainz_enabled,
            deezer_enabled: raw.deezer_enabled,
            soulseek_enabled: raw.soulseek_enabled,
            slskd_base_url,
            slskd_username,
            slskd_password,
            slskd_downloads_dir,
            music_root,
            default_quality,
            database_url,
            log_format,
            download_lyrics: raw.download_lyrics,
            download_max_parallel_tracks,
            auth,
        })
    }

    fn build_auth_config(
        auth_disabled: bool,
        session_secret: String,
        init_admin_username: Option<String>,
        init_admin_password: Option<String>,
    ) -> Result<AuthConfig, AppConfigError> {
        if !auth_disabled && session_secret.is_empty() {
            return Err(AppConfigError::Validation(
                "AUTH_SESSION_SECRET is required when AUTH_DISABLED=false".to_string(),
            ));
        }

        match (&init_admin_username, &init_admin_password) {
            (Some(_), Some(_)) | (None, None) => {}
            _ => {
                return Err(AppConfigError::Validation(
                    "AUTH_INIT_ADMIN_USERNAME and AUTH_INIT_ADMIN_PASSWORD must both be set or both be unset"
                        .to_string(),
                ));
            }
        }

        Ok(AuthConfig {
            enabled: !auth_disabled,
            session_secret,
            init_admin_username,
            init_admin_password,
        })
    }

    #[cfg(test)]
    fn from_iter<K, V, I>(iter: I) -> Result<Self, AppConfigError>
    where
        K: Into<String>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        let raw = config_parser().build_from_iter::<RawAppConfig, _, _, _>(iter)?;
        Self::from_raw(raw)
    }
}

fn config_parser() -> envious::Config<'static> {
    let mut config = envious::Config::default();
    config.case_sensitive(false);
    config
}

fn normalize_string(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_string_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_log_format(value: &str) -> String {
    match normalize_string(value, DEFAULT_LOG_FORMAT)
        .to_ascii_lowercase()
        .as_str()
    {
        "pretty" => "pretty".to_string(),
        "json" => "json".to_string(),
        _ => DEFAULT_LOG_FORMAT.to_string(),
    }
}

fn parse_quality(value: &str) -> Quality {
    normalize_string_opt(value)
        .and_then(|value| value.parse().ok())
        .unwrap_or(Quality::Lossless)
}

fn parse_optional_url(value: &str, env_var: &str) -> Result<Option<Url>, AppConfigError> {
    match normalize_string_opt(value) {
        Some(value) => parse_url(&value, env_var).map(Some),
        None => Ok(None),
    }
}

fn parse_url(value: &str, env_var: &str) -> Result<Url, AppConfigError> {
    let trimmed = value.trim().trim_end_matches('/');

    Url::parse(trimmed)
        .map_err(|err| AppConfigError::Validation(format!("{env_var} is invalid: {err}")))
}

fn default_true() -> bool {
    true
}

fn default_music_root() -> String {
    DEFAULT_MUSIC_ROOT.to_string()
}

fn default_database_url() -> String {
    DEFAULT_DATABASE_URL.to_string()
}

fn default_log_format() -> String {
    DEFAULT_LOG_FORMAT.to_string()
}

fn default_slskd_base_url() -> String {
    DEFAULT_SLSKD_BASE_URL.to_string()
}

fn default_slskd_downloads_dir() -> String {
    DEFAULT_SLSKD_DOWNLOADS_DIR.to_string()
}

fn default_quality() -> String {
    "LOSSLESS".to_string()
}

fn default_download_max_parallel_tracks() -> usize {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_defaults_for_empty_values() {
        let cfg = AppConfig::from_iter([
            ("MUSIC_ROOT", ""),
            ("DEFAULT_QUALITY", "   "),
            ("AUTH_DISABLED", "true"),
        ])
        .expect("config parse");

        assert_eq!(cfg.music_root, DEFAULT_MUSIC_ROOT);
        assert_eq!(cfg.default_quality, Quality::Lossless);
    }

    #[test]
    fn falls_back_for_invalid_quality_values() {
        let cfg = AppConfig::from_iter([
            ("DEFAULT_QUALITY", "not-real-quality"),
            ("AUTH_DISABLED", "true"),
        ])
        .expect("config parse");

        assert_eq!(cfg.default_quality, Quality::Lossless);
    }

    #[test]
    fn trims_base_url_trailing_slash() {
        let cfg = AppConfig::from_iter([
            ("TIDAL_API_BASE_URL", "http://localhost:8000///"),
            ("AUTH_DISABLED", "true"),
        ])
        .expect("config parse");

        let url: Url = "http://localhost:8000"
            .parse()
            .expect("tidal api url parse");
        assert_eq!(cfg.tidal_api_base_url, Some(url));
    }

    #[test]
    fn requires_session_secret_when_auth_enabled() {
        let err =
            AppConfig::from_iter([("AUTH_DISABLED", "false"), ("AUTH_SESSION_SECRET", "   ")])
                .expect_err("expected validation error");

        assert!(
            err.to_string()
                .contains("AUTH_SESSION_SECRET is required when AUTH_DISABLED=false")
        );
    }

    #[test]
    fn allows_missing_session_secret_when_auth_disabled() {
        let cfg = AppConfig::from_iter([("AUTH_DISABLED", "true"), ("AUTH_SESSION_SECRET", "   ")])
            .expect("config parse");

        assert!(!cfg.auth.enabled);
        assert!(cfg.auth.session_secret.is_empty());
    }

    #[test]
    fn rejects_partial_init_admin_config() {
        let err = AppConfig::from_iter([
            ("AUTH_SESSION_SECRET", "session-secret"),
            ("AUTH_INIT_ADMIN_USERNAME", "admin"),
        ])
        .expect_err("expected validation error");

        assert!(err.to_string().contains(
            "AUTH_INIT_ADMIN_USERNAME and AUTH_INIT_ADMIN_PASSWORD must both be set or both be unset"
        ));
    }
}
