use std::path::PathBuf;

use envconfig::Envconfig;

use crate::config::DEFAULT_QUALITY;

const DEFAULT_HIFI_API_BASE_URL: &str = "http://127.0.0.1:8000";
const DEFAULT_MUSIC_ROOT: &str = "./music";
const DEFAULT_DATABASE_URL: &str = "sqlite:./yoink.db?mode=rwc";
const DEFAULT_SITE_ROOT: &str = "target/site";
const DEFAULT_LOG_FORMAT: &str = "pretty";

#[derive(Debug, Clone, Envconfig)]
pub(crate) struct AppConfig {
    #[envconfig(from = "HIFI_API_BASE_URL", default = "http://127.0.0.1:8000")]
    pub(crate) hifi_api_base_url: String,

    #[envconfig(from = "MUSIC_ROOT", default = "./music")]
    pub(crate) music_root: String,

    #[envconfig(from = "DEFAULT_QUALITY", default = "LOSSLESS")]
    pub(crate) default_quality: String,

    #[envconfig(from = "DATABASE_URL", default = "sqlite:./yoink.db?mode=rwc")]
    pub(crate) database_url: String,

    #[envconfig(from = "LEPTOS_SITE_ROOT", default = "target/site")]
    pub(crate) leptos_site_root: String,

    #[envconfig(from = "LOG_FORMAT", default = "pretty")]
    pub(crate) log_format: String,

    #[envconfig(from = "DOWNLOAD_LYRICS", default = "false")]
    pub(crate) download_lyrics: bool,
}

impl AppConfig {
    pub(crate) fn from_env() -> Result<Self, envconfig::Error> {
        let mut cfg = Self::init_from_env()?;
        cfg.normalize();
        Ok(cfg)
    }

    pub(crate) fn music_root_path(&self) -> PathBuf {
        PathBuf::from(&self.music_root)
    }

    fn normalize(&mut self) {
        self.hifi_api_base_url =
            normalize_string(&self.hifi_api_base_url, DEFAULT_HIFI_API_BASE_URL)
                .trim_end_matches('/')
                .to_string();
        self.music_root = normalize_string(&self.music_root, DEFAULT_MUSIC_ROOT);
        self.default_quality = normalize_string(&self.default_quality, DEFAULT_QUALITY);
        self.database_url = normalize_string(&self.database_url, DEFAULT_DATABASE_URL);
        self.leptos_site_root = normalize_string(&self.leptos_site_root, DEFAULT_SITE_ROOT);
        self.log_format =
            normalize_string(&self.log_format, DEFAULT_LOG_FORMAT).to_ascii_lowercase();
    }
}

fn normalize_string(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn uses_defaults_for_empty_values() {
        let mut env = HashMap::new();
        env.insert("HIFI_API_BASE_URL".to_string(), "   ".to_string());
        env.insert("MUSIC_ROOT".to_string(), "".to_string());
        env.insert("DEFAULT_QUALITY".to_string(), "   ".to_string());

        let mut cfg = AppConfig::init_from_hashmap(&env).expect("config parse");
        cfg.normalize();

        assert_eq!(cfg.hifi_api_base_url, DEFAULT_HIFI_API_BASE_URL);
        assert_eq!(cfg.music_root, DEFAULT_MUSIC_ROOT);
        assert_eq!(cfg.default_quality, DEFAULT_QUALITY);
    }

    #[test]
    fn trims_base_url_trailing_slash() {
        let mut env = HashMap::new();
        env.insert(
            "HIFI_API_BASE_URL".to_string(),
            "http://localhost:8000///".to_string(),
        );

        let mut cfg = AppConfig::init_from_hashmap(&env).expect("config parse");
        cfg.normalize();

        assert_eq!(cfg.hifi_api_base_url, "http://localhost:8000");
    }
}
