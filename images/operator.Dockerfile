FROM rust:latest as builder

WORKDIR /build
COPY . .

# Build operator binary
RUN cargo build --release -p seriousum-operator

FROM debian:bookworm-slim

LABEL org.opencontainers.image.title="Seriousum Operator"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Install CA certificates for Kubernetes API
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy operator binary
COPY --from=builder /build/target/release/seriousum-operator /usr/bin/

# Create unprivileged user
RUN useradd -m -u 1000 operator

USER operator

ENV SERIOUSUM_VERSION="v0.1.0-alpha"

ENTRYPOINT ["/usr/bin/seriousum-operator"]
