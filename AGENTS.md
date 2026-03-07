# AGENTS.md

Guidance for coding agents working in `yoink`.

## Project Snapshot

- `yoink` is a Rust monorepo for a self-hosted music library manager.
- Frontend: Leptos SSR + hydration, Tailwind CSS, WASM client crate.
- Backend: Axum, Tokio, SQLx with SQLite, tracing-based logging.
- Workspace members live under `crates/`:
  - `yoink-server`: SSR server, routes, actions, providers, DB access.
  - `yoink-app`: shared Leptos UI for SSR and hydration.
  - `yoink-client`: WASM hydration entrypoint.
  - `yoink-shared`: shared models, actions, errors, helpers.
- Migrations live in `crates/yoink-server/migrations`.
- SQLx offline metadata is committed under `.sqlx/`.

## Setup

- Preferred tool bootstrap: `mise install` installs `cargo-leptos` and `sqlx-cli`.
- Copy envs with `cp .env.example .env` for local dev.
- Important envs are documented in `.env.example`.
- Default local server address is `http://127.0.0.1:3000`.

## Core Commands

### Development

- Start the full app in watch mode: `mise run dev` or `cargo leptos watch`
- Run Dockerized app: `docker compose up -d`
- Run dev stack with `slskd`: `docker compose -f compose.dev.yaml up -d`

### Build

- Production web build: `cargo leptos build --release`
- Server-only debug build: `cargo build -p yoink-server`
- WASM client build: `cargo build -p yoink-client --target wasm32-unknown-unknown`
- Full workspace build: `cargo build --workspace`

### Lint and Format

- Format all Rust code: `cargo fmt`
- Check formatting without changing files: `cargo fmt --check`
- Lint server crate strictly: `mise run lint-server`
- Lint client crate strictly: `mise run lint-client`
- Lint both configured crates: `mise run lint`
- Direct strict clippy form: `cargo clippy --package yoink-server -- -D warnings`
- Direct strict clippy form: `cargo clippy --package yoink-client -- -D warnings`

### Tests

- Run all tests in workspace: `cargo test --workspace`
- Run only server tests: `cargo test -p yoink-server`
- Run only shared crate tests: `cargo test -p yoink-shared`
- Build tests without running them: `cargo test --workspace --no-run`

### Running a Single Test

- Preferred quick form: `cargo test -p yoink-server insert_and_load_artist`
- Exact-name form: `cargo test -p yoink-server db::artists::tests::insert_and_load_artist -- --exact`
- Single async test works the same way: `cargo test -p yoink-server remove_artist_cascades`
- Single shared-model test example: `cargo test -p yoink-shared parses_quality_variants`
- To inspect exact names first: `cargo test -p yoink-server -- --list`

## Command Notes

- `cargo leptos build --release` is the production build path used in `Dockerfile`.
- `cargo leptos watch` is the main local development workflow from `README.md` and `mise.toml`.
- A validated single-test command is `cargo test -p yoink-server insert_and_load_artist`.
- `cargo fmt --check` runs clean in the current repo state.
- `cargo test --workspace --no-run` may take a while because it compiles the whole SSR/WASM workspace.

## Repository Conventions

### Imports and Module Layout

- Keep imports grouped logically: std first, external crates next, `crate::` imports last.
- Use nested imports when they improve readability, as in `use std::{sync::Arc, time::Duration};`.
- Favor explicit imports over glob imports, except for common local test helpers in test modules.
- Declare top-level modules in `main.rs`/`lib.rs`; keep feature-gated test modules adjacent.
- Keep related functionality split by domain: `actions`, `db`, `providers`, `services`, `pages`, `components`.

### Formatting

- Follow `rustfmt` defaults; do not hand-format against the formatter.
- The codebase uses trailing commas in multiline literals and calls; keep them.
- Long chains are usually split one method per line.
- Section dividers using comment banners are common in larger files; preserve the local style.
- Doc comments are used for public or non-obvious APIs; avoid noisy comments for obvious code.

### Types and Data Modeling

- Prefer concrete domain structs/enums in `yoink-shared` for data crossing server/client boundaries.
- Derive `Debug`, `Clone`, `Serialize`, and `Deserialize` where values cross server function boundaries.
- Use `Uuid` for persistent entity identifiers.
- Use small helper constructors on error enums instead of repeating string assembly at call sites.
- Prefer enums over stringly typed state when the state is internal and finite.
- Use `Option<T>` for partial provider data and nullable DB/UI fields.

### Naming

- Types and enums: `UpperCamelCase`.
- Functions, modules, and file names: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Leptos components use `UpperCamelCase` function names and usually end with `Page`, `Dialog`, `Panel`, or similar UI nouns.
- Boolean fields should read clearly, e.g. `monitored`, `acquired`, `wanted`, `explicit`.
- Prefer descriptive verb-led async functions like `fetch_artist_bio`, `sync_artist_albums`, `remove_artist`.

### Error Handling

- On the server, prefer `AppResult<T>` / `AppError` for application flows.
- For shared server-function failures, map to `yoink_shared::YoinkError` or `ServerFnError` as appropriate.
- Add contextual error variants instead of flattening everything into generic strings.
- Include operation/resource context in errors (`operation`, `resource`, `reason`, `service`, `path`).
- Use `?` for propagation in normal flows.
- Reserve `expect`/`unwrap` for tests, startup invariants, or truly impossible states.
- Log recoverable failures with `tracing` instead of silently swallowing them.

### Async and Concurrency

- Tokio is the async runtime; prefer async-first APIs in server code.
- Shared mutable state is commonly stored behind `Arc<RwLock<_>>` or `Arc<Notify>`.
- Clone cheap handles like `AppState`, `reqwest::Client`, senders, and `Arc`s instead of fighting lifetimes.
- Background loops should log failures and continue when safe, as in reconciliation/download workers.

### Database and SQLx

- Use `sqlx::query!` / `query_as!` macros for typed SQLite queries.
- Keep SQL close to the DB function that owns it.
- DB helpers usually return `Result<_, sqlx::Error>` and let higher layers map to `AppError`.
- Schema changes require matching `.up.sql` and `.down.sql` migrations under `crates/yoink-server/migrations`.
- Because `.sqlx/` is committed and Docker builds with `SQLX_OFFLINE=true`, keep SQLx metadata in sync when queries change.

### Leptos and UI

- Use `#[component]` for UI components and `#[server(...)]` for server functions.
- Shared UI primitives live in `crates/yoink-app/src/components`.
- Common Tailwind class strings are extracted into constants; reuse existing style constants before inventing new ones.
- Tailwind-heavy UI is normal here; prefer readable constants over giant inline strings when patterns repeat.
- Preserve the repo's glassy light/dark aesthetic and existing component vocabulary.
- Use `Resource`, signals, and `StoredValue` patterns consistently with nearby code.

### Logging and Observability

- Use `tracing::{debug, info, warn, error}` macros, not `println!`.
- Log enough context to debug provider, album, artist, job, or request issues.
- Respect `LOG_FORMAT`; logging is configured centrally in `crates/yoink-server/src/logging.rs`.

### Testing Style

- Most tests are inline unit tests near the owning module under `#[cfg(test)]`.
- Async tests use `#[tokio::test]`.
- Reuse helpers from `crates/yoink-server/src/test_helpers.rs` for DB/state/provider setup.
- Prefer focused test names that describe the behavior, e.g. `remove_artist_cascades`.
- Use in-memory SQLite and tempdirs for server-side tests when possible.

## Agent Do / Don't

- Do make small, local changes that match adjacent style.
- Do update tests when changing behavior.
- Do check whether a change affects both SSR and hydrated client paths.
- Do preserve provider-specific fallbacks and partial-data handling.
- Don't introduce a Node-based workflow for app development; this repo is Rust-first.
- Don't bypass existing shared models/actions when wiring new UI-server interactions.
- Don't delete `.sqlx/` metadata or migrations casually.
- Don't replace structured tracing/error types with ad-hoc strings.

## Suggested Validation After Changes

- Default for small Rust changes: `cargo fmt --check && cargo test -p <relevant-crate>`
- For server-side behavior changes: `cargo test -p yoink-server`
- For shared model changes: `cargo test -p yoink-shared`
- For UI or full-stack changes: `mise run lint && cargo test --workspace`
- For release-sensitive changes: `cargo leptos build --release`
