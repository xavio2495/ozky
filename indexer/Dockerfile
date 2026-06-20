# Multi-stage build for the ozky indexer (Cloud Run target).
# Stage 1: compile the release binary.
FROM rust:1-trixie AS build
WORKDIR /src
# Cache deps: copy manifests first, fetch, then sources.
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

# Stage 2: slim runtime. Cloud Run sends $PORT (default 8080); main.rs reads it.
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/ozky-indexer /usr/local/bin/ozky-indexer
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ozky-indexer"]
