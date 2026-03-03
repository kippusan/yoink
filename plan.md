# Refactor Plan: Track/Album-Level Library Views & Selective Monitoring

## Overview

This refactor introduces:

1. **Unified Library page** with Artists/Albums/Tracks views (replacing the current Artists page)
2. **Track-level monitoring** — monitor individual tracks without monitoring the whole album
3. **Lightweight (unmonitored) artists** — auto-created when adding a track/album directly, with lazy discography sync
4. **Provider-level album/track search** — find and add albums or tracks directly from providers
5. **Track-level downloads** — download worker respects per-track monitoring
6. **Hierarchical Wanted page** — artist > album > track tree view

---

## Phase 1: Data Model Changes

### 1a. New DB columns & migration

**`artists` table** — add `monitored` boolean (default `1` for existing rows):

- `true` = fully monitored artist (current behavior, full discography synced)
- `false` = lightweight artist (only explicitly-added albums exist, no auto-sync)

**`tracks` table** — add `monitored` and `acquired` columns:

- `monitored INTEGER NOT NULL DEFAULT 0` — user wants this track
- `acquired INTEGER NOT NULL DEFAULT 0` — file exists on disk
- (wanted is derived: `monitored && !acquired`)

**Files:**

- New migration SQL file
- `yoink-shared/src/models.rs` — add `monitored: bool` to `MonitoredArtist`, add `monitored: bool`, `acquired: bool` to `TrackInfo`
- `yoink-server/src/db/artists.rs` — update `load_artists`, `upsert_artist` queries
- `yoink-server/src/db/tracks.rs` — update `load_tracks_for_album`, `upsert_track` queries

### 1b. Monitoring semantics shift

Current: `album.monitored` means "download all tracks". New:

- `album.monitored = true` → all tracks on this album are wanted (current behavior, "album-level monitor")
- `album.monitored = false` but some `track.monitored = true` → only those specific tracks are wanted
- An album is "partially wanted" if any track is `monitored && !acquired`

`update_wanted` in `services/library/mod.rs` becomes:

```rust
fn update_wanted(album: &mut MonitoredAlbum) {
    album.wanted = album.monitored && !album.acquired;
    // partially_wanted computed separately when needed
}
```

**Files:**

- `yoink-shared/src/models.rs` — consider adding `partially_wanted: bool` to `MonitoredAlbum` (or compute dynamically in UI)
- `yoink-shared/src/helpers.rs` — update helper functions
- `yoink-server/src/services/library/mod.rs` — update `update_wanted`

### 1c. Artist monitoring flag

**Files:**

- `yoink-shared/src/models.rs` — add `monitored: bool` to `MonitoredArtist`
- `yoink-shared/src/actions.rs` — add `ToggleArtistMonitor` variant
- `yoink-server/src/actions.rs` — handle `ToggleArtistMonitor` (toggled on → trigger `sync_artist_albums`; toggled off → stop auto-syncing)
- `yoink-server/src/services/library/sync.rs` — respect `artist.monitored` before auto-syncing

---

## Phase 2: Provider Search Extensions

### 2a. Extend `MetadataProvider` trait

Add two new optional methods with default no-op implementations:

```rust
async fn search_albums(&self, query: &str) -> Result<Vec<ProviderAlbum>, ProviderError> {
    Ok(vec![])
}

async fn search_tracks(&self, query: &str) -> Result<Vec<ProviderTrack>, ProviderError> {
    Ok(vec![])
}
```

**Files:**

- `yoink-server/src/providers/mod.rs` — add methods to trait, add `SearchAlbumResult` / `SearchTrackResult` types (or reuse `ProviderAlbum`/`ProviderTrack` with artist info attached)
- `yoink-server/src/providers/tidal/mod.rs` — implement album/track search
- `yoink-server/src/providers/deezer/mod.rs` — implement album/track search
- `yoink-server/src/providers/musicbrainz/mod.rs` — implement album search (MusicBrainz has release search)
- `yoink-server/src/providers/registry.rs` — add `search_albums_all`, `search_tracks_all` fan-out methods

### 2b. New shared result types

**Files:**

- `yoink-shared/src/models.rs` — add `SearchAlbumResult` and `SearchTrackResult` structs (with artist name/id, provider, cover, etc.)

### 2c. New server actions

**Files:**

- `yoink-shared/src/actions.rs` — add:
  - `AddAlbum { provider, external_album_id, external_artist_id, artist_name, ... }` — adds album + lightweight artist
  - `AddTrack { provider, external_track_id, external_album_id, external_artist_id, artist_name, ... }` — adds track + album + lightweight artist
  - `ToggleTrackMonitor { track_id, monitored }` — toggle individual track monitoring
  - `ToggleArtistMonitor { artist_id, monitored }` — promote/demote artist monitoring
- `yoink-server/src/actions.rs` — implement handlers for all new action variants

### 2d. New API routes and server functions

**Files:**

- `yoink-server/src/routes.rs` — add `/api/search/albums?q=...`, `/api/search/tracks?q=...`
- `yoink-server/src/server_context.rs` — add `search_albums`/`search_tracks` function pointers to `ServerContext`
- `yoink-shared/src/context.rs` — update `ServerContext` struct
- `yoink-app/src/actions.rs` — (uses existing `dispatch_action` pattern, no change needed for new actions)

---

## Phase 3: Download Worker Changes

### 3a. Track-level download support

The download worker currently downloads all tracks for an album. It needs to:

1. Check which tracks are monitored when `album.monitored = false`
2. Only download those specific tracks
3. Track completion per-track (set `track.acquired = true` after each track)
4. Album is `acquired` only when all monitored tracks are acquired

**Files:**

- `yoink-server/src/services/downloads/mod.rs` — update `enqueue_album_download` to also work for track-level downloads; potentially rename to `enqueue_download`
- `yoink-server/src/services/downloads/worker.rs` — in `download_album_job`, filter `provider_tracks` to only monitored tracks when album is not fully monitored; update `track.acquired` per-track in DB
- `yoink-server/src/services/library/mod.rs` — update `update_wanted` and reconciliation logic
- `yoink-server/src/services/library/reconcile.rs` — check track-level file existence, update `track.acquired`
- `yoink-server/src/db/tracks.rs` — add `update_track_flags` (for monitored/acquired)

---

## Phase 4: UI — Unified Library Page

### 4a. Rename Artists page → Library page with tabs

Replace `/artists` route with `/library` (keep `/artists` as redirect for compatibility). Add three views: **Artists**, **Albums**, **Tracks**.

**Files:**

- `yoink-app/src/lib.rs` — update routes: `/library` (with sub-views), redirect `/artists` → `/library`
- `yoink-app/src/components/sidebar.rs` — rename "Artists" to "Library"
- New file: `yoink-app/src/pages/library.rs` — unified library page with tab switcher

### 4b. Artists tab (refactor existing)

Mostly the existing artists page, but:

- Add visual distinction between monitored (full sync) and unmonitored (lightweight) artists
- Add "Promote to Monitored" action for lightweight artists

**Files:**

- `yoink-app/src/pages/artists.rs` — refactor into a component used by the library page; add monitored badge

### 4c. Albums tab (new view)

A grid/list of all albums across all artists, with:

- Search bar (local filter + provider search)
- Sort: A-Z, Newest, Oldest, Recently Added, By Artist
- Filter: All / Monitored / Wanted / Acquired
- Album cards show: cover, title, artist name, status badges
- Click → album detail page (existing)
- Add from search results (creates lightweight artist if needed)

**Files:**

- New file: `yoink-app/src/pages/library_albums.rs`
- `yoink-app/src/pages/mod.rs` — register new module

### 4d. Tracks tab (new view)

A list/table of all tracks across all albums, with:

- Search bar (local filter + provider search)
- Sort: A-Z, By Album, By Artist, Duration, Recently Added
- Filter: All / Monitored / Wanted / Acquired
- Track rows show: title, artist, album, duration, status
- Monitor toggle per track
- Click → album detail page (scrolled to track)
- Add from search results

**Files:**

- New file: `yoink-app/src/pages/library_tracks.rs`
- `yoink-app/src/pages/mod.rs` — register new module
- New API route/server function to load all tracks (currently tracks are only loaded per-album)
- `yoink-server/src/routes.rs` — add `/api/library/tracks` endpoint
- `yoink-server/src/db/tracks.rs` — add `load_all_tracks` or `load_tracks_with_filter` query

---

## Phase 5: Wanted Page Overhaul

### 5a. Hierarchical tree view

Replace the current flat artist-grouped list with a collapsible tree:

- **Artist** (collapsible)
  - **Album** (collapsible) — shows "full album" if album-level monitored, or individual tracks
    - **Track** — individual wanted tracks

Show both fully-wanted albums and individually-wanted tracks.

**Files:**

- `yoink-app/src/pages/wanted.rs` — rewrite with tree structure
- `yoink-shared/src/helpers.rs` — add helper to build the hierarchical wanted structure
- Server data: need tracks loaded alongside albums for the wanted view
- `yoink-server/src/routes.rs` or server functions — may need a new endpoint that returns wanted items with track detail

---

## Phase 6: Album Detail Page Updates

### 6a. Track-level monitoring on album detail

The existing tracklist table on the album detail page gets:

- Monitor toggle per track (checkbox or button)
- Acquired/wanted status indicator per track
- "Monitor All" / "Unmonitor All" buttons for the track list

**Files:**

- `yoink-app/src/pages/album_detail.rs` — add per-track monitor toggles, status indicators
- Existing `ToggleAlbumMonitor` continues to work for "monitor all tracks" behavior

---

## Phase 7: Artist Detail Page Updates

### 7a. Lightweight artist indicator + promote action

- Show a badge/indicator when an artist is "unmonitored" (lightweight)
- Add "Sync Full Discography" / "Monitor Artist" button that promotes them to fully monitored
- Disable "Sync Albums" button for unmonitored artists (or make it the promote action)

**Files:**

- `yoink-app/src/pages/artist_detail.rs` — add monitored indicator, promote button

---

## Implementation Order

```
Phase 1 (Data model)           ← foundation everything else builds on
  │
  ├── Phase 3 (Download worker) ← track-level download support
  │
  └── Phase 2 (Provider search)  ← can run in parallel with Phase 3
        │
        v
Phase 6 (Album detail track toggles) ← first visible UI for track monitoring
  │
  v
Phase 4 (Unified Library page)  ← the main UI change
  │
  v
Phase 5 (Wanted page overhaul)  ← depends on track-level data being available
  │
  v
Phase 7 (Artist detail updates) ← polish
```

---

## Key Design Decisions

| Decision | Choice |
| --- | --- |
| Single track add → album granularity | Full album stored, only selected tracks monitored for download |
| Library UI structure | Unified `/library` page with Artists/Albums/Tracks tabs |
| Provider search | Extended to support album and track search |
| Artist auto-creation | Lightweight (unmonitored) artist, lazy discography sync |
| Download granularity | Track-level (only monitored tracks downloaded) |
| Unmonitored artist behavior | Lazy sync — remaining albums fetched on demand |
| Wanted page | Hierarchical tree: artist > album > track |

---

## Migration Safety

- All new columns have defaults (`monitored DEFAULT 0` for tracks, `monitored DEFAULT 1` for artists to preserve existing behavior)
- Existing fully-monitored albums continue working unchanged
- The `wanted` derivation logic is backward-compatible
