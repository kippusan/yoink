# ── Stage 1: Chef — compute recipe ───────────────────────────
FROM rust:1.93 AS chef

RUN apt-get update -y && \
    apt-get install -y --no-install-recommends binaryen && \
    rm -rf /var/lib/apt/lists/* && \
    curl -fsSL https://github.com/cargo-bins/cargo-binstall/releases/latest/download/cargo-binstall-x86_64-unknown-linux-musl.tgz \
    | tar -xz -C /usr/local/cargo/bin && \
    cargo binstall cargo-chef cargo-leptos -y && \
    rustup target add wasm32-unknown-unknown

WORKDIR /app

# ── Stage 2: Planner — generate the dependency recipe ───────
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: Builder — cache deps, then build ───────────────
FROM chef AS builder

# Leptos env vars must be set before cooking so that dependency fingerprints
# match what cargo-leptos will use, preventing unnecessary recompilation.
ENV LEPTOS_OUTPUT_NAME=yoink
ENV LEPTOS_SITE_ROOT=target/site
ENV LEPTOS_SITE_PKG_DIR=pkg
ENV LEPTOS_SITE_ADDR=127.0.0.1:3000
ENV LEPTOS_RELOAD_PORT=3001
ENV LEPTOS_LIB_DIR=crates/yoink-client
ENV LEPTOS_BIN_DIR=crates/yoink-server
ENV LEPTOS_JS_MINIFY=true
ENV LEPTOS_HASH_FILES=false
ENV SQLX_OFFLINE=true

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --package yoink-server --no-default-features --recipe-path recipe.json

RUN cargo chef cook --release --package yoink-client --no-default-features --target wasm32-unknown-unknown --target-dir target/front --recipe-path recipe.json

COPY . .

RUN cargo leptos build --release -vv

# ── Stage 4: Runtime ─────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update -y && \
    apt-get install -y --no-install-recommends ca-certificates gosu && \
    apt-get autoremove -y && \
    apt-get clean -y && \
    rm -rf /var/lib/apt/lists/*

COPY docker-entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

WORKDIR /app

COPY --from=builder /app/target/release/yoink-server ./yoink-server
COPY --from=builder /app/target/site ./site
COPY --from=builder /app/Cargo.toml ./Cargo.toml

# better-config expects a .env file to exist
RUN touch .env

ENV PUID=1000
ENV PGID=1000
ENV MUSIC_ROOT=/music
ENV DATABASE_URL=sqlite:/data/yoink.db?mode=rwc
ENV LEPTOS_SITE_ROOT=site
ENV LEPTOS_SITE_ADDR=0.0.0.0:3000
ENV DEFAULT_QUALITY=LOSSLESS
ENV LOG_FORMAT=pretty

EXPOSE 3000

VOLUME ["/data", "/music"]

ENTRYPOINT ["/entrypoint.sh"]
CMD ["./yoink-server"]
