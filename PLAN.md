# Yoink — Multi-Source Architecture Plan

## Goal

Replace the current Tidal-only architecture with a pluggable provider system so
Yoink can pull metadata and download audio from multiple services. This is the
foundational refactor required for Yoink to eventually replace Lidarr.

---

## Current State

Yoink is a full-stack Rust/Leptos music library manager that downloads from Tidal
via a hifi-api proxy. The codebase is **deeply coupled to Tidal at every layer**:

- **Database**: `artists.id` and `albums.id` are Tidal's `i64` IDs used as primary
  keys (no surrogate key). Columns named `tidal_url`. Quality defaults are Tidal
  strings (`LOSSLESS`, `HI_RES_LOSSLESS`).
- **Domain types** (`yoink-shared`): `MonitoredArtist`, `MonitoredAlbum`, `TrackInfo`,
  `DownloadJob`, `SearchArtistResult`, and `ServerAction` all use `i64` IDs.
- **API client** (`services/hifi.rs`): 6 hifi-api endpoints + 2 uptime feed URLs
  hardcoded.
- **Download pipeline** (`services/downloads/`): BTS and DASH manifest parsing,
  `HI_RES_LOSSLESS` quality fallback logic, FLAC/M4A container detection — all
  Tidal-specific.
- **Image proxy** (`routes.rs`): Proxies `resources.tidal.com` with Tidal's
  hex-dash image ID format.
- **UI** (`yoink-app`): Routes use `i64` params, hardcoded "Tidal" labels and icons,
  `tidal.com` profile URLs.

---

## Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary key type | UUID v7 (BLOB in SQLite, 16 bytes) | Time-ordered, sqlx-native, no collisions across providers |
| Primary metadata provider | None — all providers are equal | User searches all enabled providers; no single source of truth |
| Artist-Provider relationship | Many-to-many via junction tables | One local artist can link to N Tidal IDs (duplicates), N providers |
| Tracks | Persisted in DB | Needed for cross-provider ISRC matching and offline metadata |
| Quality model | Internal enum (`lossy` / `lossless` / `hires`) | Each provider maps its own quality strings to this |
| Provider architecture | Two traits: `MetadataProvider` + `DownloadSource` | Some providers do both (Tidal), some only metadata (MusicBrainz, Deezer), some only download (SoulSeek) |
| Deezer | Metadata only (public API) | No download support planned |

### Provider Matrix

| Provider | `MetadataProvider` | `DownloadSource` |
|---|---|---|
| Tidal | Yes | Yes |
| MusicBrainz | Yes | No |
| Deezer | Yes (public API) | No |
| SoulSeek | No | Yes |

---

## Target Database Schema

All primary keys are UUID v7 stored as BLOB (16 bytes). External IDs from
providers are stored as TEXT in junction tables.

```sql
-- ═══════════════════════════════════════════════════════════════════
-- Core entities (provider-agnostic)
-- ═══════════════════════════════════════════════════════════════════

CREATE TABLE artists (
    id          BLOB PRIMARY KEY,        -- UUID v7
    name        TEXT NOT NULL,
    image_url   TEXT,                     -- best resolved image URL
    added_at    TEXT NOT NULL             -- RFC 3339
);

CREATE TABLE albums (
    id              BLOB PRIMARY KEY,    -- UUID v7
    artist_id       BLOB NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    album_type      TEXT,                -- "album", "ep", "single", "compilation"
    release_date    TEXT,
    cover_url       TEXT,
    explicit        INTEGER NOT NULL DEFAULT 0,
    monitored       INTEGER NOT NULL DEFAULT 0,
    acquired        INTEGER NOT NULL DEFAULT 0,
    wanted          INTEGER NOT NULL DEFAULT 0,
    added_at        TEXT NOT NULL
);

CREATE TABLE tracks (
    id              BLOB PRIMARY KEY,    -- UUID v7
    album_id        BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    disc_number     INTEGER NOT NULL DEFAULT 1,
    track_number    INTEGER NOT NULL,
    duration_secs   INTEGER,
    explicit        INTEGER NOT NULL DEFAULT 0,
    isrc            TEXT                  -- cross-provider identifier
);

-- ═══════════════════════════════════════════════════════════════════
-- Provider links (many-to-many)
-- ═══════════════════════════════════════════════════════════════════

CREATE TABLE artist_provider_links (
    id              BLOB PRIMARY KEY,    -- UUID v7
    artist_id       BLOB NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,        -- "tidal", "musicbrainz", "deezer"
    external_id     TEXT NOT NULL,        -- provider's ID as string
    external_url    TEXT,
    external_name   TEXT,                 -- name as it appears on the provider
    image_ref       TEXT,                 -- provider-specific image reference
    UNIQUE(provider, external_id)
);

CREATE TABLE album_provider_links (
    id              BLOB PRIMARY KEY,    -- UUID v7
    album_id        BLOB NOT NULL REFERENCES albums(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    external_url    TEXT,
    external_title  TEXT,
    cover_ref       TEXT,                 -- provider-specific cover art reference
    UNIQUE(provider, external_id)
);

CREATE TABLE track_provider_links (
    id              BLOB PRIMARY KEY,    -- UUID v7
    track_id        BLOB NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    UNIQUE(provider, external_id)
);

-- ═══════════════════════════════════════════════════════════════════
-- Download jobs
-- ═══════════════════════════════════════════════════════════════════

CREATE TABLE download_jobs (
    id               BLOB PRIMARY KEY,   -- UUID v7
    album_id         BLOB NOT NULL REFERENCES albums(id),
    source           TEXT NOT NULL,       -- download source: "tidal", "soulseek"
    album_title      TEXT NOT NULL,       -- denormalized for display
    artist_name      TEXT NOT NULL,       -- denormalized for display
    status           TEXT NOT NULL DEFAULT 'queued',
    quality          TEXT NOT NULL DEFAULT 'lossless',
    total_tracks     INTEGER NOT NULL DEFAULT 0,
    completed_tracks INTEGER NOT NULL DEFAULT 0,
    error            TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

-- ═══════════════════════════════════════════════════════════════════
-- Indexes
-- ═══════════════════════════════════════════════════════════════════

CREATE INDEX idx_albums_artist ON albums(artist_id);
CREATE INDEX idx_tracks_album ON tracks(album_id);
CREATE INDEX idx_artist_links_artist ON artist_provider_links(artist_id);
CREATE INDEX idx_artist_links_provider ON artist_provider_links(provider, external_id);
CREATE INDEX idx_album_links_album ON album_provider_links(album_id);
CREATE INDEX idx_album_links_provider ON album_provider_links(provider, external_id);
CREATE INDEX idx_track_links_track ON track_provider_links(track_id);
CREATE INDEX idx_track_links_provider ON track_provider_links(provider, external_id);
CREATE INDEX idx_jobs_album ON download_jobs(album_id);
CREATE INDEX idx_jobs_status ON download_jobs(status);
CREATE INDEX idx_tracks_isrc ON tracks(isrc);
```

### Schema Changes from Current

| Current | New | Notes |
|---|---|---|
| `artists.id INTEGER` (Tidal ID) | `artists.id BLOB` (UUID v7) | Tidal ID moves to `artist_provider_links.external_id` |
| `artists.picture TEXT` | `artists.image_url TEXT` | Resolved URL instead of Tidal image ref |
| `artists.tidal_url TEXT` | Removed | Now in `artist_provider_links.external_url` |
| `artists.quality_profile TEXT` | Removed | Moves to per-album or global config (future) |
| `albums.id INTEGER` (Tidal ID) | `albums.id BLOB` (UUID v7) | Tidal ID moves to `album_provider_links.external_id` |
| `albums.cover TEXT` | `albums.cover_url TEXT` | Resolved URL instead of Tidal image ref |
| `albums.tidal_url TEXT` | Removed | Now in `album_provider_links.external_url` |
| No tracks table | `tracks` + `track_provider_links` | New |
| `download_jobs.id INTEGER AUTOINCREMENT` | `download_jobs.id BLOB` (UUID v7) | |
| `download_jobs.album_id INTEGER` | `download_jobs.album_id BLOB` | References new UUID |
| `download_jobs.artist_id INTEGER` | Removed | Derive via `albums.artist_id`; `artist_name` denormalized |
| `download_jobs.quality` = `"LOSSLESS"` | `download_jobs.quality` = `"lossless"` | Normalized internal enum |
| No `download_jobs.source` | `download_jobs.source TEXT` | Which download provider to use |

---

## Provider Traits

Defined in a new `providers/` module within `yoink-server`:

```rust
// ── Quality ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Lossy,      // ~320kbps MP3/AAC
    Lossless,   // CD 16-bit/44.1kHz FLAC
    HiRes,      // 24-bit hi-res FLAC/MQA
}

// ── Metadata provider ──────────────────────────────────────────

/// A provider that can search for and retrieve music catalog metadata.
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    /// Unique identifier for this provider (e.g. "tidal", "musicbrainz").
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Search for artists by name.
    async fn search_artists(&self, query: &str)
        -> Result<Vec<ProviderArtist>, ProviderError>;

    /// Fetch all albums for an artist by external ID.
    async fn get_artist_albums(&self, external_artist_id: &str)
        -> Result<Vec<ProviderAlbum>, ProviderError>;

    /// Fetch all tracks for an album by external ID.
    async fn get_album_tracks(&self, external_album_id: &str)
        -> Result<Vec<ProviderTrack>, ProviderError>;

    /// Resolve a public URL for an artist profile page.
    fn artist_url(&self, external_id: &str) -> Option<String>;

    /// Resolve a public URL for an album page.
    fn album_url(&self, external_id: &str) -> Option<String>;

    /// Resolve a full image URL from a provider-specific image reference.
    fn image_url(&self, image_ref: &str, size: u32) -> Option<String>;
}

// ── Download source ────────────────────────────────────────────

/// A provider that can download audio files.
#[async_trait]
pub trait DownloadSource: Send + Sync {
    /// Unique identifier (e.g. "tidal", "soulseek").
    fn id(&self) -> &str;

    /// Resolve playback/download info for a track.
    async fn resolve_playback(
        &self,
        external_track_id: &str,
        quality: Quality,
    ) -> Result<PlaybackInfo, ProviderError>;

    /// Which qualities this source supports.
    fn supported_qualities(&self) -> &[Quality];
}

// ── Shared provider types ──────────────────────────────────────

pub struct ProviderArtist {
    pub external_id: String,
    pub name: String,
    pub image_ref: Option<String>,
    pub url: Option<String>,
}

pub struct ProviderAlbum {
    pub external_id: String,
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover_ref: Option<String>,
    pub url: Option<String>,
    pub explicit: bool,
}

pub struct ProviderTrack {
    pub external_id: String,
    pub title: String,
    pub disc_number: u32,
    pub track_number: u32,
    pub duration_secs: Option<u32>,
    pub isrc: Option<String>,
    pub explicit: bool,
}

pub enum PlaybackInfo {
    /// Direct download URL (Tidal BTS, etc.)
    DirectUrl(String),
    /// DASH segments to concatenate
    DashSegments(Vec<String>),
    /// SoulSeek peer transfer handle
    PeerTransfer { /* TBD */ },
}
```

---

## Artist Addition Flow

When a user searches for an artist:

1. Search **all enabled metadata providers** in parallel (Tidal, MusicBrainz,
   Deezer).
2. Present unified results grouped by provider with dedup hints (matching names).
3. User picks a result -> creates a local `artists` row (UUID v7) +
   `artist_provider_links` row for the chosen provider.
4. On the artist detail page, a **"Link from other providers"** button lets the
   user search other providers and attach additional `artist_provider_links`.
5. Album syncing pulls from **all linked metadata providers**, deduplicating into
   local `albums` rows.

---

## Target Directory Structure

```
crates/yoink-server/src/
├── main.rs
├── app_config.rs              -- env config (expanded for new providers)
├── config.rs                  -- constants
├── db.rs                      -- rewritten with UUID schema
├── logging.rs
├── models.rs                  -- server-only response types (search results, etc.)
├── routes.rs                  -- API routes (UUIDs in paths, provider-aware image proxy)
├── state.rs                   -- AppState (holds provider registry)
├── ui/
│   └── assets.rs              -- provider-aware asset URL resolution
├── providers/
│   ├── mod.rs                 -- trait definitions, Quality enum, ProviderError
│   ├── registry.rs            -- ProviderRegistry: holds all enabled providers
│   ├── tidal/
│   │   ├── mod.rs             -- TidalProvider: impl MetadataProvider + DownloadSource
│   │   ├── api.rs             -- hifi-api HTTP client (extracted from hifi.rs)
│   │   ├── instances.rs       -- uptime feed discovery, failover (extracted from hifi.rs)
│   │   ├── models.rs          -- HifiResponse, HifiAlbum, etc. (moved from models.rs)
│   │   └── manifest.rs        -- BTS/DASH parsing (moved from downloads/manifest.rs)
│   ├── musicbrainz/
│   │   ├── mod.rs             -- MusicBrainzProvider: impl MetadataProvider
│   │   ├── api.rs             -- MB API client (rate-limited, 1 req/sec)
│   │   └── models.rs          -- MB response types
│   ├── deezer/
│   │   ├── mod.rs             -- DeezerProvider: impl MetadataProvider
│   │   ├── api.rs             -- Deezer public API client
│   │   └── models.rs          -- Deezer response types
│   └── soulseek/
│       ├── mod.rs             -- SoulSeekSource: impl DownloadSource
│       └── ...
└── services/
    ├── mod.rs
    ├── library.rs             -- provider-agnostic library management
    └── downloads/
        ├── mod.rs             -- enqueue, worker loop (source-aware)
        ├── worker.rs          -- per-album download (uses DownloadSource trait)
        ├── io.rs              -- file I/O (unchanged, mostly generic)
        ├── metadata.rs        -- audio tagging (unchanged, mostly generic)
        └── lyrics.rs          -- lyrics fetching (unchanged)
```

### What Moves Where

| Current Location | New Location | Notes |
|---|---|---|
| `services/hifi.rs` (search, `hifi_get_json`) | `providers/tidal/api.rs` | Generic HTTP client for hifi-api |
| `services/hifi.rs` (instance discovery) | `providers/tidal/instances.rs` | Uptime feeds, failover, caching |
| `models.rs` (Hifi* types) | `providers/tidal/models.rs` | All Tidal deserialization types |
| `services/downloads/manifest.rs` | `providers/tidal/manifest.rs` | BTS/DASH are Tidal-specific |
| `config.rs` (UPTIME_FEEDS) | `providers/tidal/mod.rs` | Tidal-specific constants |
| `config.rs` (DEFAULT_QUALITY) | `providers/mod.rs` | Normalized quality enum |
| `routes.rs` (`proxy_tidal_image`) | `routes.rs` (provider-aware proxy) | Dispatch to correct provider's image URL |
| `ui/assets.rs` | `ui/assets.rs` | Uses provider links for URL resolution |

---

## Shared Types Changes (`yoink-shared`)

The shared crate types need updating for UUID-based IDs and provider-agnosticism:

```rust
// IDs become String (UUID v7 serialized as hyphenated string for JSON transport)
pub struct MonitoredArtist {
    pub id: String,                     // was i64
    pub name: String,
    pub image_url: Option<String>,      // was picture: Option<String>
    pub added_at: DateTime<Utc>,
    // tidal_url removed — available via provider links
    // quality_profile removed — future: per-album or global setting
}

pub struct MonitoredAlbum {
    pub id: String,                     // was i64
    pub artist_id: String,              // was i64
    pub title: String,
    pub album_type: Option<String>,
    pub release_date: Option<String>,
    pub cover_url: Option<String>,      // was cover: Option<String>
    pub explicit: bool,
    pub monitored: bool,
    pub acquired: bool,
    pub wanted: bool,
    pub added_at: DateTime<Utc>,
    // tidal_url removed
}

pub struct TrackInfo {
    pub id: String,                     // was i64
    pub title: String,
    pub version: Option<String>,
    pub disc_number: u32,               // new
    pub track_number: u32,
    pub duration_secs: u32,
    pub duration_display: String,
    pub isrc: Option<String>,           // new
}

pub struct DownloadJob {
    pub id: String,                     // was u64
    pub album_id: String,              // was i64
    pub source: String,                 // new: "tidal", "soulseek"
    pub album_title: String,
    pub artist_name: String,            // was artist_id: i64
    pub status: DownloadStatus,
    pub quality: String,                // now: "lossless", "hires", "lossy"
    pub total_tracks: usize,
    pub completed_tracks: usize,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Provider link info for the UI
pub struct ProviderLink {
    pub provider: String,               // "tidal", "musicbrainz", "deezer"
    pub external_id: String,
    pub external_url: Option<String>,
    pub external_name: Option<String>,
}

pub struct SearchArtistResult {
    pub provider: String,               // new: which provider returned this
    pub external_id: String,            // was id: i64
    pub name: String,
    pub image_url: Option<String>,      // was picture: Option<String>
    pub url: Option<String>,
}

// ServerAction variants updated for UUID IDs
pub enum ServerAction {
    ToggleAlbumMonitor { album_id: String, monitored: bool },
    BulkMonitor { artist_id: String, monitored: bool },
    SyncArtistAlbums { artist_id: String },
    RemoveArtist { artist_id: String, remove_files: bool },
    AddArtist {                             // reworked
        name: String,
        provider: String,                   // which provider this came from
        external_id: String,                // provider's ID
        image_url: Option<String>,
        external_url: Option<String>,
    },
    LinkArtistProvider {                     // new
        artist_id: String,
        provider: String,
        external_id: String,
        external_url: Option<String>,
        external_name: Option<String>,
        image_ref: Option<String>,
    },
    UnlinkArtistProvider {                   // new
        artist_id: String,
        provider: String,
        external_id: String,
    },
    CancelDownload { job_id: String },
    ClearCompleted,
    RetryDownload { album_id: String },
    RemoveAlbumFiles { album_id: String, unmonitor: bool },
    RetagLibrary,
    ScanImportLibrary,
}
```

---

## New Dependencies

Add to workspace `Cargo.toml`:

```toml
uuid = { version = "1", features = ["v7", "serde"] }
async-trait = "0.1"
```

Enable the `uuid` feature on `sqlx`:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "uuid"] }
```

---

## Implementation Phases

### Phase 1: UUID + Schema Migration

**Goal**: Replace Tidal `i64` IDs with UUID v7 BLOBs, create the new schema with
junction tables and tracks table.

**Files to change**:
- `Cargo.toml` — add `uuid`, `async-trait`; add `uuid` feature to `sqlx`
- `crates/yoink-shared/Cargo.toml` — add `uuid` dependency
- `crates/yoink-shared/src/lib.rs` — update all types to use `String` for IDs
  (UUIDs serialized as strings for JSON transport between server and WASM client)
- `crates/yoink-server/src/db.rs` — rewrite schema creation with new tables,
  rewrite all CRUD functions
- `crates/yoink-server/src/models.rs` — update re-exports, server-only types
- `crates/yoink-server/src/state.rs` — update `AppState` types

**No migration from old schema.** This is a clean break — users delete the old
`yoink.db` and start fresh. The old schema is too Tidal-coupled to migrate
meaningfully, and the app is early enough that this is acceptable.

---

### Phase 1b: Update All DB Queries

**Goal**: All CRUD functions work with the new schema.

**Files to change**:
- `crates/yoink-server/src/db.rs` — new functions:
  - `upsert_artist_provider_link`, `delete_artist_provider_link`,
    `load_artist_provider_links`
  - `upsert_album_provider_link`, `delete_album_provider_link`,
    `load_album_provider_links`
  - `upsert_track`, `load_tracks_for_album`, `upsert_track_provider_link`
  - All existing functions updated for `Uuid` parameters

**All callers** of `db::*` functions need updating — primarily:
- `services/library.rs` — all album/artist ID references
- `services/downloads/mod.rs` — job creation, album lookups
- `services/downloads/worker.rs` — job progress updates
- `main.rs` (`dispatch_action_impl`) — all action handlers

---

### Phase 2: Define Provider Traits

**Goal**: Create the trait abstractions and shared types in a new `providers/` module.

**New files**:
- `crates/yoink-server/src/providers/mod.rs` — `MetadataProvider`, `DownloadSource`
  traits, `Quality` enum, `ProviderError`, shared types (`ProviderArtist`,
  `ProviderAlbum`, `ProviderTrack`, `PlaybackInfo`)

**No existing code changes** — this is purely additive.

---

### Phase 2b: Extract Tidal Provider

**Goal**: Move all Tidal-specific code into `providers/tidal/` and implement both
traits.

**Files to create**:
- `providers/tidal/mod.rs` — `TidalProvider` struct, impl `MetadataProvider` +
  `DownloadSource`
- `providers/tidal/api.rs` — `hifi_get_json`, HTTP client logic
  (from `services/hifi.rs`)
- `providers/tidal/instances.rs` — uptime feed discovery, failover, `InstanceCache`
  (from `services/hifi.rs`)
- `providers/tidal/models.rs` — all `Hifi*` types (from `models.rs`)
- `providers/tidal/manifest.rs` — BTS/DASH parsing
  (from `services/downloads/manifest.rs`)

**Files to delete after extraction**:
- `services/hifi.rs` — functionality moved to `providers/tidal/`
- `services/downloads/manifest.rs` — moved to `providers/tidal/manifest.rs`

**Files to change**:
- `models.rs` — remove all `Hifi*` types, keep server-only response types
  (`SearchResultArtist`, `SearchQuery`)
- `config.rs` — remove `UPTIME_FEEDS`, `DEFAULT_QUALITY` (moved to provider)
- `state.rs` — `AppState` gains a provider registry, loses `manual_hifi_base_url`,
  `instance_cache`
- `services/mod.rs` — update re-exports

---

### Phase 2c: Provider Registry

**Goal**: A central registry that holds all enabled providers and dispatches
operations.

**New files**:
- `providers/registry.rs`:
  ```rust
  pub struct ProviderRegistry {
      metadata: Vec<Arc<dyn MetadataProvider>>,
      download: Vec<Arc<dyn DownloadSource>>,
  }

  impl ProviderRegistry {
      /// Fan-out search to all metadata providers concurrently.
      pub async fn search_artists_all(&self, query: &str)
          -> Vec<(String, Vec<ProviderArtist>)>;

      /// Get a specific download source by ID.
      pub fn download_source(&self, id: &str)
          -> Option<Arc<dyn DownloadSource>>;

      /// Get a specific metadata provider by ID.
      pub fn metadata_provider(&self, id: &str)
          -> Option<Arc<dyn MetadataProvider>>;

      /// List all enabled metadata provider IDs.
      pub fn metadata_provider_ids(&self) -> Vec<String>;

      /// List all enabled download source IDs.
      pub fn download_source_ids(&self) -> Vec<String>;
  }
  ```

**Files to change**:
- `state.rs` — `AppState` holds `Arc<ProviderRegistry>`
- `main.rs` — construct providers from config, build registry, inject into state
- `app_config.rs` — add env vars for enabling/disabling providers and
  provider-specific config (e.g. `TIDAL_ENABLED`, `TIDAL_API_BASE_URL`,
  `DEEZER_ENABLED`, `MUSICBRAINZ_ENABLED`)

---

### Phase 3: UI Decoupling

**Goal**: Frontend uses UUID string IDs, shows provider badges, supports the new
search and linking flows.

**Files to change** (all in `crates/yoink-app/src/`):
- `lib.rs` — route params stay as `:id` strings (same pattern, different semantics)
- `pages/artists.rs`:
  - Search results grouped by provider with provider badges/icons
  - "Add" creates local artist + initial provider link
- `pages/artist_detail.rs`:
  - Show linked providers section with icons and external links
  - "Link from other providers" button opens a search dialog scoped to a specific
    provider
  - Remove hardcoded Tidal icon and URL
- `pages/album_detail.rs`:
  - Show linked providers
  - Remove hardcoded Tidal link
- `pages/wanted.rs` — update for String ID types
- `pages/dashboard.rs` — update for String ID types
- `components/` — update for new types

**Files to change** (in `crates/yoink-shared/src/`):
- `lib.rs` — remove `tidal_image_url`, `album_profile_url`,
  `monitored_artist_profile_url`, `search_artist_profile_url`, etc. Replace with
  generic helpers that take provider links. Remove all `tidal.com` hardcoded URLs.

**Files to change** (in `crates/yoink-server/src/`):
- `routes.rs` — image proxy becomes provider-aware:
  `/api/image/{provider}/{ref}/{size}`. Each provider resolves its own image URL
  format via `MetadataProvider::image_url()`.
- `ui/assets.rs` — rewrite to use provider links for URL resolution

---

### Phase 3b: Update Download Pipeline

**Goal**: Download worker uses `DownloadSource` trait instead of hardcoded hifi-api
calls.

**Files to change**:
- `services/downloads/mod.rs`:
  - `enqueue_album_download` takes a `source: &str` parameter
  - `DownloadJob` includes `source` field
  - Worker resolves download source from registry
- `services/downloads/worker.rs`:
  - `download_album_job` resolves the download source from the provider registry
  - Calls `resolve_playback()` via the `DownloadSource` trait instead of
    `hifi_get_json` directly
  - Track list fetched from local DB `tracks` table (populated during album sync)
  - Cover art fetched from `albums.cover_url` (already resolved)
- `services/downloads/io.rs` — mostly unchanged (generic file I/O). The
  `DownloadPayload` / `PlaybackInfo` enum handling may need adjustment.
- `services/downloads/metadata.rs`:
  - `fetch_cover_art_bytes` uses `cover_url` from albums table (already a full
    URL, no Tidal image ref conversion needed)
  - `fetch_track_info_extra` (Tidal-specific: `/info/` endpoint for ISRC, BPM,
    key, etc.) moves into the Tidal provider. Generic extra metadata comes from
    the `tracks` table.

---

### Phase 4: MusicBrainz Metadata Provider

**Goal**: Implement `MetadataProvider` for MusicBrainz.

**New files**:
- `providers/musicbrainz/mod.rs` — `MusicBrainzProvider` impl
- `providers/musicbrainz/api.rs` — MB API client
- `providers/musicbrainz/models.rs` — MB response types

**API endpoints**:
- Artist search: `GET /ws/2/artist?query={name}&fmt=json`
- Release groups (albums): `GET /ws/2/release-group?artist={mbid}&fmt=json`
- Release (tracks): `GET /ws/2/release/{mbid}?inc=recordings&fmt=json`
- Cover art: `GET https://coverartarchive.org/release/{mbid}/front-500`
- ISRC lookup: `GET /ws/2/isrc/{isrc}?fmt=json`

**Rate limiting**: MusicBrainz requires max 1 request/second with a meaningful
User-Agent header (per their ToS). Use a semaphore or token bucket internally.

---

### Phase 5: Deezer Metadata Provider

**Goal**: Implement `MetadataProvider` for Deezer's public API (metadata only, no
downloads).

**New files**:
- `providers/deezer/mod.rs` — `DeezerProvider` impl
- `providers/deezer/api.rs` — Deezer public API client (no auth required)
- `providers/deezer/models.rs` — Deezer response types

**API endpoints**:
- Artist search: `GET https://api.deezer.com/search/artist?q={name}`
- Artist albums: `GET https://api.deezer.com/artist/{id}/albums`
- Album tracks: `GET https://api.deezer.com/album/{id}/tracks`
- Track detail (ISRC): `GET https://api.deezer.com/track/{id}`
- Cover art: `album.cover_xl` field (1000x1000) directly from API response

**Rate limiting**: Deezer limits to 50 requests per 5 seconds. Use a token bucket.

---

### Phase 6: SoulSeek Download Source

**Goal**: Implement `DownloadSource` for SoulSeek.

**New files**:
- `providers/soulseek/mod.rs` — `SoulSeekSource` impl

**Architecture options**:
1. **slskd REST API** (recommended): Integrate with
   [slskd](https://github.com/slskd/slskd) as a backend service, similar to how
   Tidal uses hifi-api. slskd exposes a REST API for search and download.
2. **Native Rust client**: Implement the SoulSeek protocol directly. More complex,
   more self-contained.

**Challenges**:
- Search is fuzzy — results may not match the exact album
- Quality is variable and must be detected from the downloaded file
- Download is P2P with no guaranteed availability
- Need matching heuristics (artist name + album title + track title similarity)

**This phase needs more design work before implementation.**

---

### Phase 7: Cross-Provider Matching

**Goal**: Automatically link entities across providers using ISRC codes and fuzzy
matching.

**Implementation**:
- When tracks from different providers share the same ISRC, automatically suggest
  linking their parent albums and artists.
- Fuzzy matching fallback: normalize artist name + album title, compare similarity
  scores.
- UI: show "potential matches" on artist/album detail pages, let user confirm or
  dismiss.
- Manual linking is always available via the "Link from other providers" button.

---

## Migration Path for Existing Users

**No automatic migration.** The old database schema is too Tidal-coupled to
migrate meaningfully. Users should:

1. Delete `yoink.db` (and `yoink.db-shm`, `yoink.db-wal` if present).
2. Start the new version — a fresh database is created automatically.
3. Re-add artists via the UI (search and add from any provider).
4. Use "Scan & Import Library" to re-discover existing music files on disk.
   The folder structure (`{artist}/{album} ({date})/`) is provider-agnostic
   and will be matched against the newly synced catalog.

---

## Open Questions

- **Quality profiles**: Should quality preferences be per-artist, per-album, or
  global? Currently per-artist (`quality_profile` column) but rarely varied.
  Suggestion: global default with per-album override.
- **Download source selection**: When an album is linked to multiple download
  sources, how to choose? Suggestion: user-configured priority order
  (e.g., Tidal > SoulSeek), with manual override per album.
- **SoulSeek architecture**: Use slskd REST API as a backend (like hifi-api for
  Tidal) or embed a native Rust SoulSeek client? slskd is simpler but adds a
  deployment dependency.
- **Image caching**: Should cover art and artist images be cached on disk instead
  of proxied on every request? Would reduce external requests and survive provider
  outages.
