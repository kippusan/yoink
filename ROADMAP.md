# Roadmap

This roadmap is not a set in stone plan or guarantee for the features just a place to dump ideas with realistic goals of implementation.

No time estimates, since I don't know when I will have time to move this further along.

If you have an idea that's not on the roadmap, feel free to open an issue and we can discuss it there.
If you want to contribute and implement a feature, even better!

## Alpha Releases (`0.1.x`)

### Assumptions

- Only early adopter, moderately to highly technical
- **"Developer UX"**: the ux may not seem logical, flows are not ironed out etc.
- People don't import their main music collection
- Database drops may happen

### Goals

- [x] Download stuff
- [x] Search stuff
- [x] Tag stuff
- [x] slskd integration
- [x] Multiple providers
- [x] Simple web ui
- [x] Import only from folder structure created by yoink
- [x] Quality settings for downloads (e.g. prefer FLAC if available)
- [x] Deezer metadata support
- [x] MusicBrainz metadata support
- [ ] Support mixes in addition to albums and singles
- [ ] SoundCloud download support (yt-dlp)
- [ ] YouTube download support (yt-dlp)
- [x] Tidal metadata / download support
- [ ] Automatic metadata enrichment (e.g. if an album is missing a release date, try to find it on MusicBrainz or other sources)
- [x] Automatic album art fetching and embedding in files
- [x] Lyrics fetching and embedding in files / sidecars
- [x] Mobile-friendly web UI
- [ ] API for third-party integrations and custom scripts
- [ ] Changelog with all changes, especially breaking ones, documented clearly

## Beta Releases (`0.x.y` where x >= 2)

### Assumptions

- Early adopters / power users who are not necessarily technical
- UX: flows are ironed out, the app is usable without reading docs or watching videos
- People may import their main music collection, but they are aware of the risks and have backups
- Database drops should be avoided, but some data loss may happen
- Some breaking changes may happen, but they will be communicated in advance and documented in the changelog
- The app is still in active development, so expect some rough edges and incomplete features
- The app is not yet feature complete, so some features may be missing or not fully implemented

### Goals

- [ ] Import from existing folder structures (e.g. iTunes, other media managers, manually organized folders)
- [ ] Playlist support (import from m3u, export to m3u)
- [ ] Import playlist from streaming services (e.g. Spotify, Tidal, Deezer, YouTube Music)
- [ ] OIDC Authentication support
- [ ] Update notifier (e.g. a banner in the UI when a new version is available with a link to the changelog)
- [ ] Scrobbling support (e.g. Last.fm / ListenBrainz)
- [ ] Transcoding support (saving space without quality loss, e.g. converting WAV to FLAC)
- [ ] deduplication (must be careful with this one, as it can lead to data loss if not implemented correctly)

## Stable Releases (`1.0.x`)

### Assumptions

- General users who can deploy arr stack apps can use the app without issues
- UX: polished and intuitive, no rough edges, flows are smooth and logical
- People can safely import their main music collection without risking data loss or corruption
- Database is stable and reliable, with proper backup and recovery mechanisms in place (support for external databases like Postgres or Turso)
- Breaking changes are avoided, but if they are necessary they will be clearly communicated and documented in the changelog with migration guides
- The app is feature complete, with all major features implemented and working as intended

### Goals

_I'll fill this one once we're in beta_

## Far Future / Wishlist

- [ ] Local AI based recommendations and discovery (e.g. "find me more music like this", "what's a good album to listen to if I like this one")
- [ ] Integration with home assistants (e.g. Alexa, Google Home, Home Assistant)

## Features that won't be implemented

_I want to keep the scope of the project reasonable, so if there are features that are not planned they will go here._
_Some of these may be reconsidered in the future if there is enough demand and they fit within the scope of the project._

- Spotify metadata support
  The Spotify API is gated behind their premium plan, and I'm not subscribed to it.
  Even if I were, it would require either me providing a client id that could be blocked at any time, or users having to create their own Spotify app which has been a barrier for some other projects.
  If someone wants to implement this, I would be open to reconsidering this.
- Youtube Music metadata support
  The YouTube Music API is not public and is subject to change at any time, which makes it a risky dependency.
  I would consider implementing this if there is a stable and reliable way to access the data, but for now it's not a priority.
  yt-dlp does not really support metadata fetching it's focused on downloading via direct URLs and searches
- SoundCloud metadata support
  Similar to Spotify, their API is gated behind a SC app. I'm more open to reconsidering this one since it would be nice to add support for mixes and DJ sets, but it's still not a priority for now.
