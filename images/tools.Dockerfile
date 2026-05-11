FROM rust:latest as builder

WORKDIR /build
COPY . .

# Build CLI tools
RUN cargo build --release \
    -p seriousum-cli \
    -p seriousum-dbg

FROM debian:bookworm-slim

LABEL org.opencontainers.image.title="Seriousum Tools"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Install helpful tools
RUN apt-get update && \
    apt-get install -y \
      ca-certificates \
      curl \
      jq \
      bash && \
    rm -rf /var/lib/apt/lists/*

# Copy CLI binaries
COPY --from=builder /build/target/release/seriousum-cli /usr/bin/cilium
COPY --from=builder /build/target/release/cilium-dbg /usr/bin/cilium-dbg

# Create symlinks
RUN ln -sf /usr/bin/cilium /usr/bin/seriousum-cli && \
    ln -sf /usr/bin/cilium-dbg /usr/bin/seriousum-dbg

ENV SERIOUSUM_VERSION="v0.1.0-alpha"

ENTRYPOINT ["/usr/bin/cilium"]
CMD ["--help"]
