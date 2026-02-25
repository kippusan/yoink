# UX Improvements

Audit of remaining UX gaps in yoink, organized by impact.

## High Impact

| # | Issue | Where | Notes |
|---|---|---|---|
| 1 | No mobile navigation — sidebar is `max-md:hidden` with no alternative | `sidebar.rs:90` | Add hamburger/drawer or bottom tab bar |
| 2 | No search debounce — every keystroke fires a Tidal API call | `artists.rs:159-168` | Add ~300ms debounce before triggering resource |
| 3 | No button loading states — buttons stay clickable during async ops, no double-click protection | all pages | Track pending signal per action, disable button + show spinner |
| 4 | Already-added artists still show "+ Add" — no "Already Monitored" indicator in search results | `artists.rs:278-312` | Cross-reference search results against monitored artists |
| 5 | 404 page is unstyled — just raw "Not found." text, no sidebar or navigation | `lib.rs:26` | Create proper NotFoundPage with sidebar and link back |
| 6 | Dashboard table overflows on mobile — 7-column table with no responsive handling | `dashboard.rs:201-220` | Wrap in `overflow-x-auto` or switch to card layout on mobile |
| 7 | No "Queue Download" button on Wanted page — can only Retry failed, no way to trigger for "not queued" albums | `wanted.rs:245-267` | Add Download/Queue button for albums with no active job |

## Medium Impact

| # | Issue | Where | Notes |
|---|---|---|---|
| 8 | No client-side filter for artist collection — hard to find artists once you have 50+ | `artists.rs:221-242` | Add filter/search input above collection grid |
| 9 | No sorting options for artists or albums | `artists.rs`, `artist_detail.rs` | Add sort dropdown (A-Z, recently added, most wanted) |
| 10 | No bulk actions on Wanted page — no "Queue All" or "Retry All Failed" | `wanted.rs` header | Add bulk action buttons |
| 11 | ConfirmDialog lacks focus trap — can tab behind modal, no auto-focus, missing `role="dialog"` | `confirm_dialog.rs` | Auto-focus Cancel button, add focus trap + ARIA attrs |
| 12 | Missing ARIA labels — icon buttons, nav, theme toggle | multiple files | Add `aria-label` to all interactive elements |
| 13 | No clear button on search input | `artists.rs:159-169` | Add X icon that resets query signal |
| 14 | No loading state for search results — Suspense fallback is empty | `artists.rs:175` | Show "Searching..." indicator |
| 15 | Stat cards look clickable but aren't — hover lift effect implies interactivity | `dashboard.rs:138-159` | Make them links or remove hover transform |
| 16 | Raw error strings shown to users — no retry button, no friendly message | all error states | Show user-friendly message + optional "Details" collapse |
| 17 | Hardcoded `take(25)` on recent activity — no pagination or "show more" | `dashboard.rs:125` | Add "Show more" or pagination, show "25 of N" |
| 18 | SSE has no reconnect/error UI — if server restarts, client silently goes stale | `hooks.rs:37-54` | Add onerror handler, show "Reconnecting..." banner |

## Low Impact

| # | Issue | Where | Notes |
|---|---|---|---|
| 19 | No keyboard shortcuts (`/` for search, `g d`/`g a`/`g w` for nav) | global | Add global key handler |
| 20 | Theme doesn't react to OS `prefers-color-scheme` changes | `sidebar.rs` | Add `matchMedia` change listener |
| 21 | Duplicated Tailwind class constants across 4 page files + 1 component | all pages | Extract into `crate::styles` module |
| 22 | No per-route page titles (always just "yoink") | `shell.rs:39` | Use `leptos_meta::Title` per page |
| 23 | No favicon | `shell.rs` | Add `<link rel="icon">` |
| 24 | Tracklist panel has dark bg even in light mode | `artist_detail.rs:486` | Use `bg-zinc-100/80 dark:bg-zinc-900/60` |
| 25 | Quality profile not displayed on artist detail | `artist_detail.rs` | Show quality_profile in header card |
| 26 | No visual distinction for "Resolving" vs "Queued" status | shared `lib.rs` | Add `status-resolving` pill class |
| 27 | No skeleton loading placeholders | all page fallbacks | Add pulsing gray placeholder shapes |
| 28 | No "No results" message when search returns empty | `artists.rs:191-206` | Show "No artists found for 'query'" |

## Suggested Order

1. **#21** — Deduplicate Tailwind constants (makes everything else easier)
2. **#2** — Search debounce (quick win, prevents API spam)
3. **#3** — Button loading states (extends existing dispatch_with_toast)
4. **#4** — Already-added indicator (small change, big usability win)
5. **#1** — Mobile navigation (significant effort but critical)
