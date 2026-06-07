# ---- build stage ----
FROM rust:1-slim AS build
WORKDIR /app

# Cache dependencies first: copy manifests, build a stub, then the real source.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ---- runtime stage ----
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# rustls is used (no OpenSSL), but we still need CA certs for HTTPS to Gemini.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/hivemind /usr/local/bin/hivemind
COPY web ./web
ENV HIVEMIND_WEB_DIR=/app/web
ENV PORT=8080
EXPOSE 8080

# GEMINI_API_KEY is optional — without it the marketplace runs in demo mode.
CMD ["hivemind"]
