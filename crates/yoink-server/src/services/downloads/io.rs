use std::path::Path;
use std::time::Duration;

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::config::DOWNLOAD_CHUNK_SIZE;

pub(crate) enum DownloadPayload {
    DirectUrl(String),
    DashSegmentUrls(Vec<String>),
}

pub(crate) async fn download_payload_to_file(
    http: &reqwest::Client,
    payload: &DownloadPayload,
    path: &Path,
) -> Result<(), String> {
    match payload {
        DownloadPayload::DirectUrl(url) => download_to_file(http, url, path).await,
        DownloadPayload::DashSegmentUrls(urls) => {
            download_dash_segments_to_file(http, urls, path).await
        }
    }
}

async fn download_to_file(http: &reqwest::Client, url: &str, path: &Path) -> Result<(), String> {
    let mut response = http
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|err| format!("download request failed: {err}"))?
        .error_for_status()
        .map_err(|err| format!("download status failed: {err}"))?;

    let mut file = fs::File::create(path)
        .await
        .map_err(|err| format!("failed creating file: {err}"))?;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("failed reading stream chunk: {err}"))?
    {
        if chunk.is_empty() {
            continue;
        }
        for slice in chunk.chunks(DOWNLOAD_CHUNK_SIZE) {
            file.write_all(slice)
                .await
                .map_err(|err| format!("failed writing file: {err}"))?;
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("failed flushing file: {err}"))?;
    Ok(())
}

async fn download_dash_segments_to_file(
    http: &reqwest::Client,
    urls: &[String],
    path: &Path,
) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .await
        .map_err(|err| format!("failed creating file: {err}"))?;

    for (idx, url) in urls.iter().enumerate() {
        let bytes = http
            .get(url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|err| format!("dash segment request failed at {idx}: {err}"))?
            .error_for_status()
            .map_err(|err| format!("dash segment status failed at {idx}: {err}"))?
            .bytes()
            .await
            .map_err(|err| format!("dash segment body failed at {idx}: {err}"))?;

        if bytes.is_empty() {
            continue;
        }
        for slice in bytes.chunks(DOWNLOAD_CHUNK_SIZE) {
            file.write_all(slice)
                .await
                .map_err(|err| format!("failed writing dash segment {idx}: {err}"))?;
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("failed flushing file: {err}"))?;
    Ok(())
}

pub(crate) async fn has_flac_stream_marker(path: &Path) -> Result<bool, String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|err| format!("failed opening file {}: {err}", path.display()))?;
    let mut header = [0u8; 4];
    let read = file
        .read(&mut header)
        .await
        .map_err(|err| format!("failed reading header {}: {err}", path.display()))?;
    Ok(read == 4 && header == *b"fLaC")
}

pub(crate) async fn sniff_media_container(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|err| format!("failed opening file {}: {err}", path.display()))?;
    let mut header = [0u8; 12];
    let read = file
        .read(&mut header)
        .await
        .map_err(|err| format!("failed reading header {}: {err}", path.display()))?;
    if read >= 4 && header[..4] == *b"fLaC" {
        return Ok("flac".to_string());
    }
    if read >= 8 && header[4..8] == *b"ftyp" {
        return Ok("mp4".to_string());
    }
    Ok("unknown".to_string())
}

pub(crate) fn sanitize_path_component(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string();

    if sanitized.is_empty() {
        "Unknown".to_string()
    } else {
        sanitized
    }
}

pub(crate) fn parse_track_number_from_path(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?.trim();
    let digits = stem
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

pub(crate) fn extract_year(release_date: &str) -> String {
    let year = release_date.chars().take(4).collect::<String>();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        year
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    // ── sanitize_path_component ─────────────────────────────────

    #[test]
    fn sanitize_normal_text_unchanged() {
        assert_eq!(sanitize_path_component("Hello World"), "Hello World");
    }

    #[test]
    fn sanitize_replaces_path_separators() {
        assert_eq!(sanitize_path_component("AC/DC"), "AC_DC");
        assert_eq!(sanitize_path_component("back\\slash"), "back_slash");
    }

    #[test]
    fn sanitize_replaces_special_chars() {
        assert_eq!(sanitize_path_component("file:name"), "file_name");
        assert_eq!(sanitize_path_component("what*ever"), "what_ever");
        assert_eq!(sanitize_path_component("who?"), "who_");
        assert_eq!(sanitize_path_component("say\"what"), "say_what");
        assert_eq!(sanitize_path_component("<tag>"), "_tag_");
        assert_eq!(sanitize_path_component("pipe|char"), "pipe_char");
    }

    #[test]
    fn sanitize_replaces_control_chars() {
        assert_eq!(sanitize_path_component("hello\x00world"), "hello_world");
        assert_eq!(sanitize_path_component("tab\there"), "tab_here");
    }

    #[test]
    fn sanitize_trims_whitespace() {
        assert_eq!(sanitize_path_component("  spaced  "), "spaced");
    }

    #[test]
    fn sanitize_empty_becomes_unknown() {
        assert_eq!(sanitize_path_component(""), "Unknown");
    }

    #[test]
    fn sanitize_only_whitespace_becomes_unknown() {
        assert_eq!(sanitize_path_component("   "), "Unknown");
    }

    #[test]
    fn sanitize_mixed_input() {
        assert_eq!(
            sanitize_path_component("Artist: The Best Of (2024)"),
            "Artist_ The Best Of (2024)"
        );
    }

    #[test]
    fn sanitize_unicode_preserved() {
        assert_eq!(sanitize_path_component("Bjork"), "Bjork");
        assert_eq!(
            sanitize_path_component("Sigur Ros"),
            "Sigur Ros"
        );
    }

    // ── parse_track_number_from_path ────────────────────────────

    #[test]
    fn parse_track_number_leading_digits() {
        assert_eq!(
            parse_track_number_from_path(Path::new("01 Song Title.flac")),
            Some(1)
        );
        assert_eq!(
            parse_track_number_from_path(Path::new("12 Another Song.m4a")),
            Some(12)
        );
    }

    #[test]
    fn parse_track_number_three_digits() {
        assert_eq!(
            parse_track_number_from_path(Path::new("101 Long Album.flac")),
            Some(101)
        );
    }

    #[test]
    fn parse_track_number_no_digits() {
        assert_eq!(
            parse_track_number_from_path(Path::new("Song Title.flac")),
            None
        );
    }

    #[test]
    fn parse_track_number_in_subdirectory() {
        assert_eq!(
            parse_track_number_from_path(Path::new("/music/artist/album/03 Track.flac")),
            Some(3)
        );
    }

    // ── extract_year ────────────────────────────────────────────

    #[test]
    fn extract_year_full_date() {
        assert_eq!(extract_year("2024-01-15"), "2024");
    }

    #[test]
    fn extract_year_just_year() {
        assert_eq!(extract_year("2024"), "2024");
    }

    #[test]
    fn extract_year_non_digit() {
        assert_eq!(extract_year("abcd-01-01"), "");
    }

    #[test]
    fn extract_year_too_short() {
        assert_eq!(extract_year("20"), "");
    }

    #[test]
    fn extract_year_empty() {
        assert_eq!(extract_year(""), "");
    }

    #[test]
    fn extract_year_mixed_prefix() {
        // "v202" has a non-digit 'v' at position 0
        assert_eq!(extract_year("v202"), "");
    }
}
