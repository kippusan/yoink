use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use chrono::Utc;
use serde::de::DeserializeOwned;
use tracing::{debug, info, warn};

use crate::{
    config::UPTIME_FEEDS,
    models::{
        DownInstance, FeedInstance, HifiArtist, HifiResponse, InstancesResponse, RankedInstance,
        UptimeFeed,
    },
    state::AppState,
};

pub(crate) async fn hifi_get_json<T: DeserializeOwned>(
    state: &AppState,
    path: &str,
    query: Vec<(String, String)>,
) -> Result<T, String> {
    let candidates = candidate_base_urls(state).await;
    let mut last_error = None;

    for base_url in candidates {
        let response = state
            .http
            .get(format!("{base_url}{path}"))
            .query(&query)
            .timeout(Duration::from_secs(8))
            .send()
            .await;

        match response {
            Ok(resp) => match resp.error_for_status() {
                Ok(ok) => match ok.json::<T>().await {
                    Ok(parsed) => {
                        set_active_instance(state, &base_url).await;
                        return Ok(parsed);
                    }
                    Err(err) => {
                        debug!(base_url, error = %err, "Upstream JSON parse failed");
                        last_error = Some(format!("{base_url}: invalid JSON ({err})"));
                    }
                },
                Err(err) => {
                    debug!(base_url, error = %err, "Upstream HTTP status failed");
                    last_error = Some(format!("{base_url}: upstream status error ({err})"));
                }
            },
            Err(err) => {
                debug!(base_url, error = %err, "Upstream request failed");
                last_error = Some(format!("{base_url}: request failed ({err})"));
            }
        }
    }

    let error_msg =
        last_error.unwrap_or_else(|| "No healthy hifi-api instances available".to_string());
    warn!(error = %error_msg, "All hifi-api candidates failed");
    Err(error_msg)
}

pub(crate) async fn search_hifi_artists(
    state: &AppState,
    query: &str,
) -> Result<Vec<HifiArtist>, String> {
    let parsed = hifi_get_json::<HifiResponse>(
        state,
        "/search/",
        vec![("a".to_string(), query.to_string())],
    )
    .await?;

    let artists = parsed
        .data
        .artists
        .map(|paged| paged.items)
        .or(parsed.data.items)
        .unwrap_or_default();

    Ok(artists)
}

async fn candidate_base_urls(state: &AppState) -> Vec<String> {
    ensure_instances_fresh(state).await;

    let mut candidates = Vec::new();
    if let Some(manual) = &state.manual_hifi_base_url {
        candidates.push(manual.clone());
    }

    {
        let cache = state.instance_cache.read().await;
        if let Some(active) = &cache.active_base_url {
            candidates.push(active.clone());
        }
        candidates.extend(cache.ranked.iter().map(|instance| instance.url.clone()));
    }

    let mut seen = HashSet::new();
    candidates.retain(|url| seen.insert(url.clone()));
    candidates
}

pub(crate) async fn ensure_instances_fresh(state: &AppState) {
    let should_refresh = {
        let cache = state.instance_cache.read().await;
        cache.is_stale()
    };

    if should_refresh {
        refresh_instances(state).await;
    }
}

async fn refresh_instances(state: &AppState) {
    debug!("Refreshing hifi instance feed cache");
    let mut merged_api = Vec::new();
    let mut merged_streaming = Vec::new();
    let mut merged_down = Vec::new();

    for feed_url in UPTIME_FEEDS {
        let send_res = state
            .http
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

    let mut cache = state.instance_cache.write().await;
    cache.last_refresh = Some(Utc::now());
    cache.last_refresh_instant = Some(Instant::now());
    cache.api = merged_api;
    cache.streaming = merged_streaming;
    cache.down = dedup_down(merged_down);
    cache.ranked = ranked;
}

async fn set_active_instance(state: &AppState, base_url: &str) {
    let mut cache = state.instance_cache.write().await;
    cache.active_base_url = Some(base_url.to_string());
}

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

pub(crate) async fn list_instances_payload(state: &AppState) -> InstancesResponse {
    ensure_instances_fresh(state).await;
    let cache = state.instance_cache.read().await;
    debug!(
        ranked = cache.ranked.len(),
        api = cache.api.len(),
        streaming = cache.streaming.len(),
        down = cache.down.len(),
        "Returning cached instance list"
    );

    InstancesResponse {
        manual_override: state.manual_hifi_base_url.clone(),
        active_base_url: cache.active_base_url.clone(),
        last_refresh: cache.last_refresh,
        ranked: cache.ranked.clone(),
        api: cache.api.clone(),
        streaming: cache.streaming.clone(),
        down: cache.down.clone(),
    }
}
