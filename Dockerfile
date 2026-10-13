# Confidant, deployed as a public read-only demo.
#
# The image contains no model, no API key and no writable clinical store: it builds the static UI
# and the release daemon, then runs the daemon with CONFIDANT_DEMO=1, which serves the recorded
# runs in `demo/` plus the newest `eval/results/*.json` and answers every POST/PATCH/DELETE with 403.
#
#   docker build -t confidant-demo .
#   docker run -e PORT=10000 -p 10000:10000 confidant-demo

# --- 1. the static UI (next build, output: 'export') --------------------------------------
FROM node:22-slim AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY ui/ ./
RUN npm run build

# --- 2. the daemon, release -----------------------------------------------------------------
# rusqlite is built with the bundled SQLite (needs a C compiler); reqwest pulls native-tls, so
# the build stage needs OpenSSL headers. No runtime API key is read at any point in the build.
FROM rust:1-slim AS daemon
RUN apt-get update \
 && apt-get install -y --no-install-recommends gcc libc6-dev libssl-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY daemon ./daemon
COPY prompts ./prompts
RUN cargo build --release --locked

# --- 3. runtime ------------------------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 app
WORKDIR /app
COPY --from=daemon /src/target/release/confidant-daemon /app/confidant-daemon
COPY --from=ui /ui/out /app/ui/out
COPY demo /app/demo
COPY eval/results /app/eval/results
# Demo mode is the deployment: recorded runs, read-only, bound to the platform's $PORT.
ENV CONFIDANT_DEMO=1 \
    CONFIDANT_ROOT=/app \
    CONFIDANT_UI=/app/ui/out \
    CONFIDANT_BIND=0.0.0.0 \
    CONFIDANT_DB=/tmp/confidant-demo.db
USER app
EXPOSE 10000
CMD ["/app/confidant-daemon"]
