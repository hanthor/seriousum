FROM rust:latest as builder

WORKDIR /build
COPY . .

# Build all release binaries
RUN cargo build --release \
    -p seriousum-daemon \
    -p seriousum-cli \
    -p seriousum-dbg

FROM quay.io/cilium/cilium:latest

LABEL org.opencontainers.image.title="Seriousum Agent"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"
LABEL org.opencontainers.image.documentation="https://github.com/hanthor/seriousum"
LABEL org.opencontainers.image.authors="Seriousum Contributors"

# Copy Rust binaries into Cilium image
COPY --from=builder /build/target/release/seriousum-daemon /opt/cilium/seriousum-daemon
COPY --from=builder /build/target/release/seriousum-cli /usr/bin/seriousum-cli
COPY --from=builder /build/target/release/cilium-dbg /usr/bin/seriousum-dbg

# Create wrapper symlinks for compatibility
RUN ln -sf /opt/cilium/seriousum-daemon /usr/bin/cilium-agent && \
    ln -sf /usr/bin/seriousum-cli /usr/bin/cilium && \
    ln -sf /usr/bin/seriousum-dbg /usr/bin/cilium-dbg

# Verify binaries
RUN /opt/cilium/seriousum-daemon --version && \
    /usr/bin/seriousum-cli --version && \
    /usr/bin/seriousum-dbg --version

ENV SERIOUSUM_VERSION="v0.1.0-alpha"

ENTRYPOINT ["/opt/cilium/seriousum-daemon"]
