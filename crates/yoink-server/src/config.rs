use std::time::Duration;

pub(crate) const INSTANCE_CACHE_TTL: Duration = Duration::from_secs(300);
pub(crate) const DOWNLOAD_CHUNK_SIZE: usize = 64 * 1024;
pub(crate) const DEFAULT_QUALITY: &str = "LOSSLESS";
pub(crate) const QUALITY_WARNING: &str = "Default quality is LOSSLESS (CD, 16-bit/44.1kHz FLAC). Set DEFAULT_QUALITY=HI_RES_LOSSLESS for 24-bit hi-res if available.";
pub(crate) const UPTIME_FEEDS: [&str; 2] = [
    "https://tidal-uptime.jiffy-puffs-1j.workers.dev/",
    "https://tidal-uptime.props-76styles.workers.dev/",
];
