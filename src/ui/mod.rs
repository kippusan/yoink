pub(crate) mod design7;

use std::collections::HashMap;

use crate::models::*;

// ── Shared image / URL helpers ──────────────────────────────────────

pub(crate) fn tidal_image_url(image_id: &str, size: u16) -> String {
    // Proxy through our server to avoid CORS/OpaqueResponseBlocking from resources.tidal.com
    format!("/api/image/{image_id}/{size}")
}

pub(crate) fn artist_image_url(artist: &HifiArtist, size: u16) -> Option<String> {
    artist
        .picture
        .as_deref()
        .or(artist.selected_album_cover_fallback.as_deref())
        .map(|id| tidal_image_url(id, size))
}

pub(crate) fn monitored_artist_image_url(artist: &MonitoredArtist, size: u16) -> Option<String> {
    artist
        .picture
        .as_deref()
        .map(|id| tidal_image_url(id, size))
}

pub(crate) fn artist_profile_url(artist: &HifiArtist) -> String {
    artist
        .url
        .clone()
        .unwrap_or_else(|| format!("https://tidal.com/artist/{}", artist.id))
}

pub(crate) fn monitored_artist_profile_url(artist: &MonitoredArtist) -> String {
    artist
        .tidal_url
        .clone()
        .unwrap_or_else(|| format!("https://tidal.com/artist/{}", artist.id))
}

pub(crate) fn album_cover_url(album: &MonitoredAlbum, size: u16) -> Option<String> {
    album.cover.as_deref().map(|id| tidal_image_url(id, size))
}

pub(crate) fn album_profile_url(album: &MonitoredAlbum) -> Option<String> {
    album
        .tidal_url
        .as_deref()
        .map(|url| url.replace("http://", "https://"))
}

pub(crate) fn status_class(status: &DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued | DownloadStatus::Resolving => "pill status-queued",
        DownloadStatus::Downloading => "pill status-downloading",
        DownloadStatus::Completed => "pill status-completed",
        DownloadStatus::Failed => "pill status-failed",
    }
}

// ── Shared scripts ──────────────────────────────────────────────────

pub(crate) fn theme_bootstrap_script() -> &'static str {
    r#"
(() => {
  const stored = localStorage.getItem('theme');
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  if (stored === 'dark' || (!stored && prefersDark)) {
    document.documentElement.classList.add('dark');
  } else {
    document.documentElement.classList.remove('dark');
  }
})();
    "#
}

pub(crate) fn theme_interaction_script() -> &'static str {
    r#"
(function () {
  function setLabel(isDark) {
    document.querySelectorAll('[data-theme-label]').forEach(function(label) {
      label.textContent = isDark ? 'Dark' : 'Light';
    });
  }

  function syncLabel() {
    setLabel(document.documentElement.classList.contains('dark'));
  }

  document.querySelectorAll('[data-theme-toggle]').forEach(function(toggle) {
    toggle.addEventListener('click', function () {
      const isDark = document.documentElement.classList.toggle('dark');
      localStorage.setItem('theme', isDark ? 'dark' : 'light');
      setLabel(isDark);
    });
  });

  syncLabel();
})();
"#
}

pub(crate) fn live_updates_script() -> &'static str {
    r#"
(function () {
  const statusClass = {
    queued: "pill status-queued",
    resolving: "pill status-queued",
    downloading: "pill status-downloading",
    completed: "pill status-completed",
    failed: "pill status-failed"
  };

  function latestJobsByAlbum(jobs) {
    const map = new Map();
    for (const job of jobs) {
      const key = String(job.album_id);
      const existing = map.get(key);
      const existingUpdated = existing ? Date.parse(existing.updated_at || "") : 0;
      const currentUpdated = Date.parse(job.updated_at || "");
      if (!existing || currentUpdated >= existingUpdated) {
        map.set(key, job);
      }
    }
    return map;
  }

  function statusLabel(job) {
    if (!job) return "No Job";
    if (job.status === "downloading") {
      const done = Number(job.completed_tracks || 0);
      const total = Number(job.total_tracks || 0);
      if (total > 0) return "Downloading " + done + "/" + total;
      return "Downloading";
    }
    if (job.status === "resolving") return "Resolving";
    if (job.status === "queued") return "Queued";
    if (job.status === "completed") return "Completed";
    if (job.status === "failed") return "Failed";
    return "Unknown";
  }

  function updateAlbumRows(jobsByAlbum, albumsById) {
    const rows = document.querySelectorAll("[data-album-row]");
    for (const row of rows) {
      const albumId = row.getAttribute("data-album-id");
      if (!albumId) continue;
      const job = jobsByAlbum.get(albumId);
      const statusEl = row.querySelector("[data-job-status]");
      if (statusEl) {
        statusEl.textContent = statusLabel(job);
        statusEl.className = statusClass[job?.status] || "pill";
      }

      const wantedEl = row.querySelector("[data-wanted-pill]");
      const album = albumsById.get(albumId);
      if (wantedEl && album) {
        wantedEl.textContent = album.wanted ? "Wanted" : "Not Wanted";
      }
    }
  }

  function updateWantedRows(jobsByAlbum, albumsById) {
    const rows = document.querySelectorAll("[data-wanted-row]");
    for (const row of rows) {
      const albumId = row.getAttribute("data-album-id");
      if (!albumId) continue;
      const job = jobsByAlbum.get(albumId);
      const album = albumsById.get(albumId);

      const statusEl = row.querySelector("[data-job-status]");
      if (statusEl) {
        statusEl.textContent = job ? (job.status || "unknown") : "not queued";
        statusEl.className = statusClass[job?.status] || "pill";
      }

      const errorEl = row.querySelector("[data-job-error]");
      const retryEl = row.querySelector("[data-retry-form]");
      if (job && job.status === "failed") {
        if (errorEl) {
          errorEl.textContent = job.error || "Download failed";
          errorEl.classList.remove("hidden");
        }
        if (retryEl) retryEl.classList.remove("hidden");
      } else {
        if (errorEl) {
          errorEl.textContent = "";
          errorEl.classList.add("hidden");
        }
        if (retryEl) retryEl.classList.add("hidden");
      }

      if ((job && job.status === "completed") || (album && !album.wanted)) {
        row.remove();
      }
    }

    const list = document.querySelector("[data-wanted-list]");
    const empty = document.querySelector("[data-wanted-empty]");
    if (list && empty && list.querySelectorAll("[data-wanted-row]").length === 0) {
      empty.classList.remove("hidden");
    }
  }

  function updateDashboard(albums, jobs) {
    const wantedCount = albums.filter((album) => album.wanted).length;
    const acquiredCount = albums.filter((album) => album.acquired).length;
    const monitoredCount = albums.filter((album) => album.monitored).length;
    const activeJobs = jobs.filter((job) => job.status === "queued" || job.status === "resolving" || job.status === "downloading").length;

    const wantedEl = document.querySelector("[data-dashboard-wanted]");
    const acquiredEl = document.querySelector("[data-dashboard-acquired]");
    const monitoredEl = document.querySelector("[data-dashboard-monitored]");
    const activeEl = document.querySelector("[data-dashboard-active-jobs]");
    if (wantedEl) wantedEl.textContent = String(wantedCount);
    if (acquiredEl) acquiredEl.textContent = String(acquiredCount);
    if (monitoredEl) monitoredEl.textContent = String(monitoredCount);
    if (activeEl) activeEl.textContent = String(activeJobs);
  }

  async function refresh() {
    try {
      const [downloadsRes, albumsRes] = await Promise.all([
        fetch("/api/downloads", { cache: "no-store" }),
        fetch("/api/library/albums", { cache: "no-store" })
      ]);
      if (!downloadsRes.ok || !albumsRes.ok) return;

      const jobs = await downloadsRes.json();
      const albums = await albumsRes.json();
      const jobsByAlbum = latestJobsByAlbum(Array.isArray(jobs) ? jobs : []);
      const albumsById = new Map();
      for (const album of Array.isArray(albums) ? albums : []) {
        albumsById.set(String(album.id), album);
      }

      updateAlbumRows(jobsByAlbum, albumsById);
      updateWantedRows(jobsByAlbum, albumsById);
      updateDashboard(Array.isArray(albums) ? albums : [], Array.isArray(jobs) ? jobs : []);
    } catch (_) {
      // ignore transient polling errors
    }
  }

  // Initial fetch
  refresh();

  // SSE-driven updates with polling fallback
  let pollTimer = null;
  function startPolling() {
    if (!pollTimer) pollTimer = setInterval(refresh, 5000);
  }
  function stopPolling() {
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  }

  function connectSSE() {
    try {
      const es = new EventSource("/api/events");
      es.addEventListener("update", function () { refresh(); });
      es.onopen = function () { stopPolling(); };
      es.onerror = function () {
        es.close();
        startPolling();
        // Retry SSE after 10s
        setTimeout(connectSSE, 10000);
      };
    } catch (_) {
      startPolling();
    }
  }

  connectSSE();
})();
"#
}

pub(crate) fn tracklist_script() -> &'static str {
    r#"
(function () {
  const cache = {};

  document.addEventListener('click', function (e) {
    const btn = e.target.closest('[data-tracklist-toggle]');
    if (!btn) return;
    e.preventDefault();
    const albumId = btn.getAttribute('data-tracklist-toggle');
    const panel = document.querySelector('[data-tracklist-panel="' + albumId + '"]');
    if (!panel) return;

    if (panel.classList.contains('open')) {
      panel.classList.remove('open');
      btn.classList.remove('active');
      return;
    }

    panel.classList.add('open');
    btn.classList.add('active');

    if (cache[albumId]) {
      renderTracks(panel, cache[albumId]);
      return;
    }

    panel.innerHTML = '<div class="d7-tracklist-loading">Loading tracks\u2026</div>';
    fetch('/api/albums/' + albumId + '/tracks', { cache: 'no-store' })
      .then(function (r) { return r.json(); })
      .then(function (tracks) {
        if (Array.isArray(tracks)) {
          cache[albumId] = tracks;
          renderTracks(panel, tracks);
        } else {
          panel.innerHTML = '<div class="d7-tracklist-loading">Failed to load tracks</div>';
        }
      })
      .catch(function () {
        panel.innerHTML = '<div class="d7-tracklist-loading">Failed to load tracks</div>';
      });
  });

  function renderTracks(panel, tracks) {
    if (tracks.length === 0) {
      panel.innerHTML = '<div class="d7-tracklist-loading">No tracks found</div>';
      return;
    }
    var html = '';
    for (var i = 0; i < tracks.length; i++) {
      var t = tracks[i];
      html += '<div class="d7-track-row">';
      html += '<span class="d7-track-num">' + t.track_number + '</span>';
      html += '<span class="d7-track-title">' + escHtml(t.title) + '</span>';
      html += '<span class="d7-track-dur">' + t.duration_display + '</span>';
      html += '</div>';
    }
    panel.innerHTML = html;
  }

  function escHtml(s) {
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }
})();
"#
}

pub(crate) fn instant_search_script() -> &'static str {
    r#"
(function () {
  var input = document.querySelector('[data-instant-search]');
  if (!input) return;
  var dropdown = document.querySelector('[data-search-dropdown]');
  if (!dropdown) return;
  var form = input.closest('form');
  var timer = null;

  input.addEventListener('input', function () {
    clearTimeout(timer);
    var q = input.value.trim();
    if (q.length < 2) {
      dropdown.classList.remove('visible');
      dropdown.innerHTML = '';
      return;
    }
    timer = setTimeout(function () { doSearch(q); }, 300);
  });

  input.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      dropdown.classList.remove('visible');
    }
  });

  document.addEventListener('click', function (e) {
    if (!dropdown.contains(e.target) && e.target !== input) {
      dropdown.classList.remove('visible');
    }
  });

  // Allow Enter to still submit the form for a full-page search
  if (form) {
    form.addEventListener('submit', function () {
      dropdown.classList.remove('visible');
    });
  }

  function doSearch(q) {
    dropdown.innerHTML = '<div class="d7-search-dropdown-loading">Searching\u2026</div>';
    dropdown.classList.add('visible');

    fetch('/api/search?q=' + encodeURIComponent(q), { cache: 'no-store' })
      .then(function (r) { return r.json(); })
      .then(function (results) {
        if (!Array.isArray(results) || results.length === 0) {
          dropdown.innerHTML = '<div class="d7-search-dropdown-loading">No results</div>';
          return;
        }
        var html = '';
        for (var i = 0; i < results.length; i++) {
          var a = results[i];
          html += '<div class="d7-search-dropdown-item">';
          if (a.picture_url) {
            html += '<img class="d7-search-dropdown-avatar" src="' + escAttr(a.picture_url) + '" alt="" />';
          } else {
            var initial = a.name.charAt(0).toUpperCase() || '?';
            html += '<div class="d7-search-dropdown-fallback">' + escHtml(initial) + '</div>';
          }
          html += '<div class="d7-search-dropdown-info">';
          html += '<div class="d7-search-dropdown-name">' + escHtml(a.name) + '</div>';
          if (a.already_monitored) {
            html += '<div class="d7-search-dropdown-hint">Already in library</div>';
          }
          html += '</div>';
          if (a.already_monitored) {
            html += '<a href="/artists/' + a.id + '" class="d7-btn d7-btn-sm" style="text-decoration:none">View</a>';
          } else {
            html += '<form action="/artists/add" method="post" style="display:inline">';
            html += '<input type="hidden" name="id" value="' + a.id + '" />';
            html += '<input type="hidden" name="name" value="' + escAttr(a.name) + '" />';
            html += '<input type="hidden" name="picture" value="' + escAttr(a.picture_url || '') + '" />';
            html += '<input type="hidden" name="tidal_url" value="' + escAttr(a.tidal_url || '') + '" />';
            html += '<input type="hidden" name="return_to" value="/artists" />';
            html += '<button type="submit" class="d7-btn d7-btn-primary d7-btn-sm">+ Add</button>';
            html += '</form>';
          }
          html += '</div>';
        }
        dropdown.innerHTML = html;
      })
      .catch(function () {
        dropdown.innerHTML = '<div class="d7-search-dropdown-loading">Search failed</div>';
      });
  }

  function escHtml(s) {
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  function escAttr(s) {
    return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }
})();
"#
}

pub(crate) fn album_glow_script() -> &'static str {
    r#"
(function () {
  function pickGlowColor(img) {
    var canvas = document.createElement('canvas');
    canvas.width = 24;
    canvas.height = 24;
    var ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;

    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
    var data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;

    var r = 0;
    var g = 0;
    var b = 0;
    var n = 0;

    for (var i = 0; i < data.length; i += 4) {
      var pr = data[i];
      var pg = data[i + 1];
      var pb = data[i + 2];
      var pa = data[i + 3];
      if (pa < 180) continue;

      var max = Math.max(pr, pg, pb);
      var min = Math.min(pr, pg, pb);
      var sat = max === 0 ? 0 : (max - min) / max;
      var lum = (pr + pg + pb) / 3;

      if (lum < 25 || lum > 235) continue;
      if (sat < 0.18) continue;

      r += pr;
      g += pg;
      b += pb;
      n += 1;
    }

    if (n === 0) return null;

    return {
      r: Math.round(r / n),
      g: Math.round(g / n),
      b: Math.round(b / n),
    };
  }

  function applyGlowFromImage(img) {
    var sleeve = img.closest('.d7-sleeve');
    if (!sleeve) return;

    try {
      var c = pickGlowColor(img);
      if (!c) return;
      sleeve.style.setProperty('--d7-glow-rgb', c.r + ', ' + c.g + ', ' + c.b);
    } catch (_) {
      // ignore color extraction issues
    }
  }

  var covers = document.querySelectorAll('.d7-sleeve-cover');
  for (var i = 0; i < covers.length; i++) {
    var img = covers[i];
    if (img.complete && img.naturalWidth > 0) {
      applyGlowFromImage(img);
    } else {
      img.addEventListener('load', function (e) {
        applyGlowFromImage(e.currentTarget);
      }, { once: true });
    }
  }
})();
"#
}

// ── Shared data preparation ─────────────────────────────────────────

pub(crate) fn build_albums_by_artist(
    albums: Vec<MonitoredAlbum>,
) -> HashMap<i64, Vec<MonitoredAlbum>> {
    let mut map: HashMap<i64, Vec<MonitoredAlbum>> = HashMap::new();
    for album in albums {
        map.entry(album.artist_id).or_default().push(album);
    }
    // Sort each artist's albums by release date descending
    for albums in map.values_mut() {
        albums.sort_by(|a, b| {
            b.release_date
                .cmp(&a.release_date)
                .then_with(|| a.title.cmp(&b.title))
        });
    }
    map
}

pub(crate) fn build_latest_jobs(jobs: Vec<DownloadJob>) -> HashMap<i64, DownloadJob> {
    let mut map: HashMap<i64, DownloadJob> = HashMap::new();
    for job in jobs {
        map.entry(job.album_id)
            .and_modify(|existing| {
                if job.updated_at > existing.updated_at {
                    *existing = job.clone();
                }
            })
            .or_insert(job);
    }
    map
}

pub(crate) fn build_artist_names(artists: &[MonitoredArtist]) -> HashMap<i64, String> {
    artists.iter().map(|a| (a.id, a.name.clone())).collect()
}
