FROM rust:1.95-bookworm AS cni-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY benches ./benches
COPY src ./src
COPY crates ./crates
RUN cargo build --release -p seriousum-cni

FROM rust:1.95-bookworm AS dbg-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY benches ./benches
COPY src ./src
COPY crates ./crates
RUN cargo build --release -p seriousum-dbg

FROM quay.io/cilium/cilium-ci:latest

LABEL org.opencontainers.image.title="Seriousum Agent"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Keep upstream base image scripts/helpers (init containers rely on them),
# but replace the agent and CNI binaries with seriousum builds. Replace cilium-dbg
# with the dedicated CLI crate artifact, not the root-package daemon wrapper.
COPY target/release/cilium /usr/bin/cilium-agent
COPY target/release/cilium-health /usr/bin/cilium-health
COPY --from=dbg-builder /build/target/release/cilium-dbg /usr/bin/cilium-dbg
COPY --from=cni-builder /build/target/release/seriousum-cni /opt/cni/bin/cilium-cni
COPY --from=cni-builder /build/target/release/seriousum-cni /usr/bin/cilium-cni
