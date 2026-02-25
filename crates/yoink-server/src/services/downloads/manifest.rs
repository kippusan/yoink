use base64::Engine;
use roxmltree::{Document, Node};
use tracing::warn;

use crate::models::{BtsManifest, HifiPlaybackData};

pub(crate) enum DownloadPayload {
    DirectUrl(String),
    DashSegmentUrls(Vec<String>),
}

pub(crate) fn extract_download_payload(playback: &HifiPlaybackData) -> Result<DownloadPayload, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(playback.manifest.as_bytes())
        .map_err(|err| format!("failed to decode manifest: {err}"))?;

    match playback.manifest_mime_type.as_str() {
        "application/vnd.tidal.bts" => {
            let manifest = serde_json::from_slice::<BtsManifest>(&decoded)
                .map_err(|err| format!("failed to parse BTS manifest: {err}"))?;
            manifest
                .urls
                .first()
                .cloned()
                .map(DownloadPayload::DirectUrl)
                .ok_or_else(|| "no track URL in BTS manifest".to_string())
        }
        "application/dash+xml" => {
            let xml = String::from_utf8(decoded)
                .map_err(|err| format!("DASH manifest is not valid UTF-8: {err}"))?;
            if let Ok(urls) = extract_dash_segment_urls(&xml)
                && !urls.is_empty()
            {
                return Ok(DownloadPayload::DashSegmentUrls(urls));
            }
            extract_dash_base_url(&xml).map(DownloadPayload::DirectUrl)
        }
        other => {
            warn!(manifest_mime_type = %other, "Unknown manifest type, attempting BTS parse as fallback");
            let manifest = serde_json::from_slice::<BtsManifest>(&decoded)
                .map_err(|err| format!("unsupported manifest type '{}': {err}", other))?;
            manifest
                .urls
                .first()
                .cloned()
                .map(DownloadPayload::DirectUrl)
                .ok_or_else(|| format!("no track URL in manifest (type: {})", other))
        }
    }
}

fn extract_dash_segment_urls(xml: &str) -> Result<Vec<String>, String> {
    let doc = Document::parse(xml).map_err(|err| format!("failed to parse DASH XML: {err}"))?;

    let mpd = doc
        .descendants()
        .find(|n| n.has_tag_name("MPD"))
        .ok_or_else(|| "DASH manifest has no MPD element".to_string())?;
    let period = mpd
        .children()
        .find(|n| n.has_tag_name("Period"))
        .ok_or_else(|| "DASH manifest has no Period element".to_string())?;

    let adaptation_sets: Vec<Node<'_, '_>> = period
        .children()
        .filter(|n| n.has_tag_name("AdaptationSet"))
        .collect();
    if adaptation_sets.is_empty() {
        return Err("DASH manifest has no AdaptationSet".to_string());
    }

    let audio_set = adaptation_sets
        .iter()
        .copied()
        .find(|set| {
            set.attribute("mimeType")
                .map(|v| v.starts_with("audio"))
                .unwrap_or(false)
                || set
                    .attribute("contentType")
                    .map(|v| v.eq_ignore_ascii_case("audio"))
                    .unwrap_or(false)
        })
        .unwrap_or(adaptation_sets[0]);

    let mut reps: Vec<Node<'_, '_>> = audio_set
        .children()
        .filter(|n| n.has_tag_name("Representation"))
        .collect();
    if reps.is_empty() {
        return Err("DASH manifest has no Representation".to_string());
    }
    reps.sort_by_key(|rep| {
        rep.attribute("bandwidth")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    });
    reps.reverse();
    let rep = reps[0];

    let rep_id = rep.attribute("id").unwrap_or("");
    let segment_template = rep
        .children()
        .find(|n| n.has_tag_name("SegmentTemplate"))
        .or_else(|| {
            audio_set
                .children()
                .find(|n| n.has_tag_name("SegmentTemplate"))
        })
        .ok_or_else(|| "DASH manifest has no SegmentTemplate".to_string())?;

    let initialization = segment_template.attribute("initialization");
    let media = segment_template
        .attribute("media")
        .ok_or_else(|| "DASH SegmentTemplate has no media template".to_string())?;
    let start_number = segment_template
        .attribute("startNumber")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);

    let base_url = rep
        .children()
        .find(|n| n.has_tag_name("BaseURL"))
        .and_then(|n| n.text())
        .or_else(|| {
            audio_set
                .children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .or_else(|| {
            period
                .children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .or_else(|| {
            mpd.children()
                .find(|n| n.has_tag_name("BaseURL"))
                .and_then(|n| n.text())
        })
        .unwrap_or("")
        .trim()
        .to_string();

    let timeline = segment_template
        .children()
        .find(|n| n.has_tag_name("SegmentTimeline"))
        .ok_or_else(|| "DASH SegmentTemplate has no SegmentTimeline".to_string())?;

    let mut entries = Vec::new();
    let mut current_time = 0u64;
    let mut current_number = start_number;
    for s in timeline.children().filter(|n| n.has_tag_name("S")) {
        if let Some(t) = s.attribute("t").and_then(|v| v.parse::<u64>().ok()) {
            current_time = t;
        }
        let duration = s
            .attribute("d")
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| "DASH timeline entry missing duration".to_string())?;
        let repeats = s
            .attribute("r")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);

        entries.push((current_number, current_time));
        current_number += 1;
        current_time += duration;

        for _ in 0..repeats.max(0) {
            entries.push((current_number, current_time));
            current_number += 1;
            current_time += duration;
        }
    }

    let mut urls = Vec::with_capacity(entries.len() + 1);
    if let Some(init) = initialization {
        let init_path = resolve_dash_template(init, rep_id, 0, 0);
        urls.push(join_dash_url(&base_url, &init_path));
    }
    for (number, time) in entries {
        let path = resolve_dash_template(media, rep_id, number, time);
        urls.push(join_dash_url(&base_url, &path));
    }

    if urls.is_empty() {
        return Err("DASH generated no segment URLs".to_string());
    }

    Ok(urls)
}

fn resolve_dash_template(template: &str, rep_id: &str, number: u64, time: u64) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let Some(end_rel) = template[i + 1..].find('$') else {
            out.push('$');
            i += 1;
            continue;
        };
        let end = i + 1 + end_rel;
        let token = &template[i + 1..end];
        if token == "RepresentationID" {
            out.push_str(rep_id);
        } else if let Some(width) = token
            .strip_prefix("Number%0")
            .and_then(|s| s.strip_suffix('d'))
        {
            let w = width.parse::<usize>().unwrap_or(0);
            if w > 0 {
                out.push_str(&format!("{number:0w$}"));
            } else {
                out.push_str(&number.to_string());
            }
        } else if token == "Number" {
            out.push_str(&number.to_string());
        } else if let Some(width) = token
            .strip_prefix("Time%0")
            .and_then(|s| s.strip_suffix('d'))
        {
            let w = width.parse::<usize>().unwrap_or(0);
            if w > 0 {
                out.push_str(&format!("{time:0w$}"));
            } else {
                out.push_str(&time.to_string());
            }
        } else if token == "Time" {
            out.push_str(&time.to_string());
        } else {
            out.push('$');
            out.push_str(token);
            out.push('$');
        }
        i = end + 1;
    }
    out
}

fn join_dash_url(base: &str, part: &str) -> String {
    if part.starts_with("http://") || part.starts_with("https://") {
        return part.to_string();
    }
    if base.is_empty() {
        return part.to_string();
    }
    if base.ends_with('/') || part.starts_with('/') {
        format!("{base}{part}")
    } else {
        format!("{base}/{part}")
    }
}

/// Extract the download URL from a DASH MPD XML manifest.
/// TIDAL DASH manifests contain `<BaseURL>` elements with the direct stream URL.
fn extract_dash_base_url(xml: &str) -> Result<String, String> {
    let mut scan_from = 0usize;
    while let Some(tag_start_rel) = xml[scan_from..].find("<BaseURL") {
        let tag_start = scan_from + tag_start_rel;
        let after_open = &xml[tag_start..];
        let Some(open_end_rel) = after_open.find('>') else {
            break;
        };
        let content_start = tag_start + open_end_rel + 1;

        let after_content = &xml[content_start..];
        let Some(close_rel) = after_content.find("</BaseURL>") else {
            scan_from = content_start;
            continue;
        };
        let raw = after_content[..close_rel].trim();
        let url = raw
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#x2F;", "/");

        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(url);
        }

        scan_from = content_start + close_rel + "</BaseURL>".len();
    }

    // Last resort: pick first absolute URL anywhere in the XML payload.
    if let Some(start) = xml.find("https://").or_else(|| xml.find("http://")) {
        let tail = &xml[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '<' || c == '"')
            .unwrap_or(tail.len());
        let candidate = tail[..end].trim();
        if candidate.starts_with("http://") || candidate.starts_with("https://") {
            return Ok(candidate.to_string());
        }
    }

    Err("no absolute URL found in DASH manifest".to_string())
}

pub(crate) fn summarize_manifest_for_logs(playback: &HifiPlaybackData) -> String {
    let decoded =
        match base64::engine::general_purpose::STANDARD.decode(playback.manifest.as_bytes()) {
            Ok(bytes) => bytes,
            Err(err) => return format!("decode_error={err}"),
        };

    if playback.manifest_mime_type != "application/dash+xml" {
        return format!(
            "mime_type={}, decoded_bytes={}",
            playback.manifest_mime_type,
            decoded.len()
        );
    }

    let xml = match String::from_utf8(decoded) {
        Ok(xml) => xml,
        Err(err) => return format!("dash_utf8_error={err}"),
    };

    let base_url_count = xml.matches("<BaseURL").count();
    let representation_count = xml.matches("<Representation").count();
    let adaptation_set_count = xml.matches("<AdaptationSet").count();
    let segment_template_count = xml.matches("<SegmentTemplate").count();
    let segment_base_count = xml.matches("<SegmentBase").count();
    let segment_list_count = xml.matches("<SegmentList").count();
    let content_protection_count = xml.matches("<ContentProtection").count();

    format!(
        "mime_type=application/dash+xml, xml_bytes={}, base_url={}, representation={}, adaptation_set={}, segment_template={}, segment_base={}, segment_list={}, content_protection={}",
        xml.len(),
        base_url_count,
        representation_count,
        adaptation_set_count,
        segment_template_count,
        segment_base_count,
        segment_list_count,
        content_protection_count
    )
}
