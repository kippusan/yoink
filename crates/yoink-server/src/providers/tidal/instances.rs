use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::models::{DownInstance, FeedInstance, InstancesResponse, RankedInstance, UptimeFeed};

const INSTANCE_CACHE_TTL: Duration = Duration::from_secs(300);

pub(crate) const UPTIME_FEEDS: [&str; 2] = [
    "https://tidal-uptime.jiffy-puffs-1j.workers.dev/",
    "https://tidal-uptime.props-76styles.workers.dev/",
];

// ── Instance cache ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct InstanceCache {
    pub last_refresh: Option<DateTime<Utc>>,
    pub last_refresh_instant: Option<Instant>,
    pub api: Vec<FeedInstance>,
    pub streaming: Vec<FeedInstance>,
    pub down: Vec<DownInstance>,
    pub ranked: Vec<RankedInstance>,
    pub active_base_url: Option<String>,
}

impl InstanceCache {
    pub fn new() -> Self {
        Self {
            last_refresh: None,
            last_refresh_instant: None,
            api: Vec::new(),
            streaming: Vec::new(),
            down: Vec::new(),
            ranked: Vec::new(),
            active_base_url: None,
        }
    }

    pub fn is_stale(&self) -> bool {
        match self.last_refresh_instant {
            Some(last) => last.elapsed() > INSTANCE_CACHE_TTL,
            None => true,
        }
    }
}

// ── Instance management functions ───────────────────────────────────

pub(crate) async fn candidate_base_urls(
    manual_base_url: Option<&str>,
    cache: &RwLock<InstanceCache>,
    http: &reqwest::Client,
) -> Vec<String> {
    ensure_instances_fresh(cache, http).await;

    let mut candidates = Vec::new();
    if let Some(manual) = manual_base_url {
        candidates.push(manual.to_string());
    }

    {
        let c = cache.read().await;
        if let Some(active) = &c.active_base_url {
            candidates.push(active.clone());
        }
        candidates.extend(c.ranked.iter().map(|instance| instance.url.clone()));
    }

    let mut seen = HashSet::new();
    candidates.retain(|url| seen.insert(url.clone()));
    candidates
}

pub(crate) async fn ensure_instances_fresh(
    cache: &RwLock<InstanceCache>,
    http: &reqwest::Client,
) {
    let should_refresh = {
        let c = cache.read().await;
        c.is_stale()
    };
    if should_refresh {
        refresh_instances(cache, http).await;
    }
}

pub(crate) async fn set_active_instance(cache: &RwLock<InstanceCache>, base_url: &str) {
    let mut c = cache.write().await;
    c.active_base_url = Some(base_url.to_string());
}

async fn refresh_instances(cache: &RwLock<InstanceCache>, http: &reqwest::Client) {
    debug!("Refreshing hifi instance feed cache");
    let mut merged_api = Vec::new();
    let mut merged_streaming = Vec::new();
    let mut merged_down = Vec::new();

    for feed_url in UPTIME_FEEDS {
        let send_res = http
            .get(feed_url)
            .timeout(Duration::from_secs(6))
            .send()
            .await;

        let Ok(resp) = send_res else {
            debug!(feed_url, "Failed to fetch uptime feed");
            continue;
        };

        let Ok(ok_resp) = resp.error_for_status() else {
            debug!(feed_url, "Uptime feed returned non-success status");
            continue;
        };

        let Ok(feed) = ok_resp.json::<UptimeFeed>().await else {
            debug!(feed_url, "Failed to parse uptime feed JSON");
            continue;
        };

        merged_api.extend(feed.api);
        merged_streaming.extend(feed.streaming);
        merged_down.extend(feed.down);
    }

    let down_set = merged_down
        .iter()
        .map(|entry| entry.url.clone())
        .collect::<HashSet<_>>();

    merged_api = dedup_instances(merged_api)
        .into_iter()
        .filter(|instance| !down_set.contains(&instance.url))
        .collect();
    merged_streaming = dedup_instances(merged_streaming)
        .into_iter()
        .filter(|instance| !down_set.contains(&instance.url))
        .collect();

    let ranked = rank_instances(&merged_streaming, &merged_api);
    info!(
        api = merged_api.len(),
        streaming = merged_streaming.len(),
        down = merged_down.len(),
        ranked = ranked.len(),
        "Refreshed hifi instance cache"
    );

    let mut c = cache.write().await;
    c.last_refresh = Some(Utc::now());
    c.last_refresh_instant = Some(Instant::now());
    c.api = merged_api;
    c.streaming = merged_streaming;
    c.down = dedup_down(merged_down);
    c.ranked = ranked;
}

pub(crate) async fn list_instances_payload(
    manual_override: Option<&str>,
    cache: &RwLock<InstanceCache>,
    http: &reqwest::Client,
) -> InstancesResponse {
    ensure_instances_fresh(cache, http).await;
    let c = cache.read().await;
    debug!(
        ranked = c.ranked.len(),
        api = c.api.len(),
        streaming = c.streaming.len(),
        down = c.down.len(),
        "Returning cached instance list"
    );
    InstancesResponse {
        manual_override: manual_override.map(String::from),
        active_base_url: c.active_base_url.clone(),
        last_refresh: c.last_refresh,
        ranked: c.ranked.clone(),
        api: c.api.clone(),
        streaming: c.streaming.clone(),
        down: c.down.clone(),
    }
}

// ── Pure helpers ────────────────────────────────────────────────────

fn dedup_instances(instances: Vec<FeedInstance>) -> Vec<FeedInstance> {
    let mut by_url: HashMap<String, FeedInstance> = HashMap::new();
    for instance in instances {
        by_url
            .entry(instance.url.clone())
            .and_modify(|existing| {
                if version_key(&instance.version) > version_key(&existing.version) {
                    *existing = instance.clone();
                }
            })
            .or_insert(instance);
    }
    let mut deduped: Vec<_> = by_url.into_values().collect();
    deduped.sort_by(|a, b| {
        version_key(&b.version)
            .cmp(&version_key(&a.version))
            .then_with(|| a.url.cmp(&b.url))
    });
    deduped
}

fn dedup_down(entries: Vec<DownInstance>) -> Vec<DownInstance> {
    let mut by_url: HashMap<String, DownInstance> = HashMap::new();
    for entry in entries {
        by_url.insert(entry.url.clone(), entry);
    }
    let mut deduped: Vec<_> = by_url.into_values().collect();
    deduped.sort_by(|a, b| a.url.cmp(&b.url));
    deduped
}

fn rank_instances(streaming: &[FeedInstance], api: &[FeedInstance]) -> Vec<RankedInstance> {
    let mut by_url: HashMap<String, RankedInstance> = HashMap::new();
    for item in api {
        by_url.insert(
            item.url.clone(),
            RankedInstance {
                url: item.url.clone(),
                version: item.version.clone(),
                source: "api".to_string(),
            },
        );
    }
    for item in streaming {
        by_url
            .entry(item.url.clone())
            .and_modify(|existing| {
                existing.source = "streaming".to_string();
                if version_key(&item.version) > version_key(&existing.version) {
                    existing.version = item.version.clone();
                }
            })
            .or_insert(RankedInstance {
                url: item.url.clone(),
                version: item.version.clone(),
                source: "streaming".to_string(),
            });
    }
    let mut ranked: Vec<_> = by_url.into_values().collect();
    ranked.sort_by(|a, b| {
        source_priority(&a.source)
            .cmp(&source_priority(&b.source))
            .then_with(|| version_key(&b.version).cmp(&version_key(&a.version)))
            .then_with(|| a.url.cmp(&b.url))
    });
    ranked
}

fn source_priority(source: &str) -> u8 {
    match source {
        "streaming" => 0,
        _ => 1,
    }
}

fn version_key(version: &str) -> (u16, u16, u16) {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}
