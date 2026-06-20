# ZK toolchain dev container for ozky Part 1 (the ZK).
# Bundles: Rust + wasm32 (Soroban), Stellar CLI, Noir (nargo), Barretenberg (bb).
# Barretenberg ships no Windows binaries, so the proving stack runs in Linux here.
# trixie (Debian 13, GCC 14) not bookworm: bb 0.87.0's release binary links
# libstdc++ GLIBCXX_3.4.32, which bookworm's GCC 12 runtime lacks.
FROM rust:1-trixie

# Pinned to match the rs-soroban-ultrahonk verifier (Nethermind + SDF): it parses
# bb 0.87.0's fixed UltraHonk-keccak proof format (456 fields). Noir beta.9 is the
# release that bb 0.87.0 was cut against. Do NOT bump without re-checking the
# verifier's proof/VK byte layout.
ARG NOIR_VERSION=1.0.0-beta.9
ARG BB_VERSION=0.87.0

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl git jq bash ca-certificates pkg-config libssl-dev \
        libc++1 libc++abi1 libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*

# Soroban builds target wasm. Stellar CLI 26.x uses wasm32v1-none; keep the
# classic target too for tooling that still expects it.
RUN rustup target add wasm32-unknown-unknown wasm32v1-none

# Stellar CLI via prebuilt binary (cargo-binstall) to avoid a long source compile.
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y stellar-cli \
    && stellar --version

# Noir (nargo), pinned.
ENV PATH="/root/.nargo/bin:${PATH}"
RUN curl -L https://raw.githubusercontent.com/noir-lang/noirup/main/install | bash \
    && /root/.nargo/bin/noirup --version ${NOIR_VERSION} \
    && ln -sf /root/.nargo/bin/nargo /usr/local/bin/nargo \
    && nargo --version

# Barretenberg (bb), pinned to 0.87.0 via the release tarball (the version the
# verifier crate's proof parser targets — not the Noir-matched bb). Mirrors the
# rs-soroban-ultrahonk build script's direct-download approach.
ENV PATH="/root/.bb:${PATH}"
RUN mkdir -p /root/.bb \
    && curl -L "https://github.com/AztecProtocol/aztec-packages/releases/download/v${BB_VERSION}/barretenberg-amd64-linux.tar.gz" -o /tmp/bb.tar.gz \
    && tar -xzf /tmp/bb.tar.gz -C /root/.bb \
    && rm /tmp/bb.tar.gz \
    && ln -sf /root/.bb/bb /usr/local/bin/bb \
    && bb --version

WORKDIR /workspace
CMD ["bash"]
