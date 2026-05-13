FROM quay.io/cilium/cilium-ci:latest

LABEL org.opencontainers.image.title="Seriousum Agent"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Keep upstream base image scripts/helpers and dbg/cli binaries (init containers rely on them),
# but replace the agent binary with seriousum build.
COPY target/release/cilium /usr/bin/cilium-agent
COPY target/release/seriousum-cni /opt/cni/bin/cilium-cni
