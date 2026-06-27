# Multi-stage build for the ozky onboarding funder. Lean (no soroban-sdk / tauri), so it
# builds about as fast as the keeper-service / indexer.
FROM rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

# Slim runtime. Cloud Run / GKE send $PORT (default 8080); main.rs binds 0.0.0.0:$PORT.
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/ozky-funder-service /usr/local/bin/ozky-funder-service
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/ozky-funder-service"]
