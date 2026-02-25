use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use lofty::{
    config::WriteOptions,
    file::{AudioFile, TaggedFileExt},
    picture::{MimeType, Picture, PictureType},
    prelude::{Accessor, ItemKey},
    probe::Probe,
    tag::{Tag, TagType},
};
use serde_json::Value;

use crate::state::AppState;

use super::io::extract_year;
use crate::services::hifi::hifi_get_json;

/// All the metadata needed to tag a single audio file.
pub(crate) struct TrackMetadata<'a> {
    pub path: &'a Path,
    pub title: &'a str,
    pub track_artist: &'a str,
    pub album_artist: &'a str,
    pub album: &'a str,
    pub track_number: u32,
    pub disc_number: Option<u32>,
    pub total_tracks: u32,
    pub release_date: &'a str,
    pub track_extra: &'a HashMap<String, Value>,
    pub album_extra: &'a HashMap<String, Value>,
    pub track_info_extra: Option<&'a HashMap<String, Value>>,
    pub lyrics_text: Option<&'a str>,
    pub cover_art_jpeg: Option<&'a [u8]>,
}

pub(crate) fn write_audio_metadata(meta: &TrackMetadata<'_>) -> Result<(), String> {
    let default_tag_type = match meta
        .path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("m4a") | Some("mp4") => TagType::Mp4Ilst,
        _ => TagType::VorbisComments,
    };

    let mut tagged_file = Probe::open(meta.path)
        .map_err(|err| err.to_string())?
        .read()
        .map_err(|err| err.to_string())?;

    let tag = if let Some(existing) = tagged_file.primary_tag_mut() {
        existing
    } else {
        tagged_file.insert_tag(Tag::new(default_tag_type));
        tagged_file
            .primary_tag_mut()
            .ok_or_else(|| "failed to create metadata tag".to_string())?
    };

    tag.set_title(meta.title.to_string());
    tag.set_artist(meta.track_artist.to_string());
    tag.set_album(meta.album.to_string());
    if !meta.album_artist.trim().is_empty() {
        tag.insert_text(ItemKey::AlbumArtist, meta.album_artist.to_string());
    }
    tag.insert_text(ItemKey::TrackNumber, meta.track_number.to_string());
    if let Some(disc) = meta.disc_number {
        tag.insert_text(ItemKey::DiscNumber, disc.to_string());
    }
    if meta.total_tracks > 0 {
        tag.insert_text(ItemKey::TrackTotal, meta.total_tracks.to_string());
    }
    let year = extract_year(meta.release_date);
    if !year.is_empty() {
        tag.insert_text(ItemKey::Year, year);
    }
    if let Some(lyrics) = meta.lyrics_text.filter(|v| !v.trim().is_empty()) {
        tag.insert_text(ItemKey::Lyrics, lyrics.to_string());
    }

    if let Some(info) = meta.track_info_extra {
        if let Some(isrc) = value_as_string(info.get("isrc")) {
            tag.insert_text(ItemKey::Isrc, isrc);
        }
        if let Some(copyright) = value_as_string(info.get("copyright")) {
            tag.insert_text(ItemKey::CopyrightMessage, copyright);
        }
        if let Some(version) = value_as_string(info.get("version"))
            && !version.trim().is_empty()
        {
            tag.insert_text(ItemKey::TrackSubtitle, version);
        }
        if let Some(initial_key) = value_as_string(info.get("key")) {
            tag.insert_text(ItemKey::InitialKey, initial_key);
        }
        if let Some(bpm) = value_as_string(info.get("bpm")) {
            tag.insert_text(ItemKey::IntegerBpm, bpm);
        }
        if let Some(track_gain) = value_as_string(info.get("trackReplayGain")) {
            tag.insert_text(ItemKey::ReplayGainTrackGain, track_gain);
        }
        if let Some(track_peak) = value_as_string(info.get("trackPeakAmplitude")) {
            tag.insert_text(ItemKey::ReplayGainTrackPeak, track_peak);
        }
        if let Some(album_gain) = value_as_string(info.get("albumReplayGain")) {
            tag.insert_text(ItemKey::ReplayGainAlbumGain, album_gain);
        }
        if let Some(album_peak) = value_as_string(info.get("albumPeakAmplitude")) {
            tag.insert_text(ItemKey::ReplayGainAlbumPeak, album_peak);
        }
    }

    if let Some(jpeg) = meta.cover_art_jpeg {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Jpeg),
            None,
            jpeg.to_vec(),
        ));
    }

    if default_tag_type == TagType::VorbisComments {
        write_extra_vorbis(tag, "TIDAL_TRACK_", meta.track_extra);
        write_extra_vorbis(tag, "TIDAL_ALBUM_", meta.album_extra);
        if let Some(info) = meta.track_info_extra {
            write_extra_vorbis(tag, "TIDAL_INFO_", info);
        }
    }

    tagged_file
        .save_to_path(meta.path, WriteOptions::default())
        .map_err(|err| err.to_string())
}

fn write_extra_vorbis(tag: &mut Tag, prefix: &str, extra: &HashMap<String, Value>) {
    for (key, value) in extra {
        if let Some(text) = value_to_text(value) {
            let key = sanitize_vorbis_key(prefix, key);
            if !key.is_empty() {
                tag.insert_text(ItemKey::Unknown(key), text);
            }
        }
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

fn value_as_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn build_full_artist_string(
    title: &str,
    track_extra: &HashMap<String, Value>,
    track_info_extra: Option<&HashMap<String, Value>>,
    fallback_artist: &str,
) -> String {
    let mut artists = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();

    {
        let mut push_artist = |name: &str| push_unique_artist(name, &mut artists, &mut seen);

        collect_artists_from_map(track_extra, &mut push_artist);
        if let Some(extra) = track_info_extra {
            collect_artists_from_map(extra, &mut push_artist);
        }
        for featured in parse_featured_artists(title) {
            push_artist(&featured);
        }
    }

    if artists.is_empty() {
        push_unique_artist(fallback_artist, &mut artists, &mut seen);
    }

    artists.join("; ")
}

fn push_unique_artist(name: &str, artists: &mut Vec<String>, seen: &mut HashSet<String>) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return;
    }
    let key = trimmed.to_ascii_lowercase();
    if seen.insert(key) {
        artists.push(trimmed.to_string());
    }
}

fn collect_artists_from_map(map: &HashMap<String, Value>, push: &mut dyn FnMut(&str)) {
    for key in ["artists", "artist"] {
        if let Some(value) = map.get(key) {
            collect_artist_names(value, push);
        }
    }
}

fn collect_artist_names(value: &Value, push: &mut dyn FnMut(&str)) {
    match value {
        Value::String(s) => push(s),
        Value::Array(items) => {
            for item in items {
                collect_artist_names(item, push);
            }
        }
        Value::Object(obj) => {
            if let Some(Value::String(name)) = obj.get("name") {
                push(name);
            } else if let Some(Value::String(name)) = obj.get("title") {
                push(name);
            }
        }
        _ => {}
    }
}

fn parse_featured_artists(title: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let lower = title.to_ascii_lowercase();
    while let Some(open_rel) = lower[start..].find('(') {
        let open = start + open_rel;
        let Some(close_rel) = lower[open + 1..].find(')') else {
            break;
        };
        let close = open + 1 + close_rel;
        let inner = title[open + 1..close].trim();
        let inner_lower = inner.to_ascii_lowercase();
        let markers = ["feat.", "feat", "ft.", "ft", "with "];
        if let Some(marker) = markers.iter().find(|m| inner_lower.starts_with(**m)) {
            let raw = inner[marker.len()..].trim();
            for piece in raw.split(',') {
                for p in piece.split('&') {
                    let name = p.trim();
                    if !name.is_empty() {
                        out.push(name.to_string());
                    }
                }
            }
        }
        start = close + 1;
    }
    out
}

pub(crate) fn extract_disc_number(
    track_extra: &HashMap<String, Value>,
    track_info_extra: Option<&HashMap<String, Value>>,
) -> Option<u32> {
    for key in ["volumeNumber", "discNumber", "volume_number", "disc_number"] {
        if let Some(val) = track_info_extra
            .and_then(|m| m.get(key))
            .or_else(|| track_extra.get(key))
        {
            match val {
                Value::Number(n) => {
                    if let Some(v) = n.as_u64().and_then(|v| u32::try_from(v).ok()) {
                        return Some(v);
                    }
                }
                Value::String(s) => {
                    if let Ok(v) = s.trim().parse::<u32>() {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn sanitize_vorbis_key(prefix: &str, key: &str) -> String {
    let normalized = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    format!("{}{}", prefix, normalized)
}

pub(crate) async fn fetch_cover_art_bytes(
    http: &reqwest::Client,
    cover_id: Option<&str>,
) -> Option<Vec<u8>> {
    let cover_id = cover_id?;
    let url = format!(
        "https://resources.tidal.com/images/{}/1080x1080.jpg",
        cover_id.replace('-', "/")
    );

    let resp = http
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;

    resp.bytes().await.ok().map(|b| b.to_vec())
}

pub(crate) async fn fetch_track_info_extra(
    state: &AppState,
    track_id: i64,
) -> Option<HashMap<String, Value>> {
    let response = hifi_get_json::<Value>(
        state,
        "/info/",
        vec![("id".to_string(), track_id.to_string())],
    )
    .await
    .ok()?;

    let data = response.get("data")?.as_object()?;
    Some(
        data.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<HashMap<String, Value>>(),
    )
}
