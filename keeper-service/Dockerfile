# Multi-stage build for the ozky cloud keeper (Cloud Run target). Lean (no soroban-sdk / tauri),
# so it builds about as fast as the indexer.
FROM rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

# Slim runtime. Cloud Run sends $PORT (default 8080); main.rs binds 0.0.0.0:$PORT.
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/ozky-keeper-service /usr/local/bin/ozky-keeper-service
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ozky-keeper-service"]
