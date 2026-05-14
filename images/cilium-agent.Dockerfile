FROM quay.io/cilium/cilium-ci:latest

LABEL org.opencontainers.image.title="Seriousum Agent"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Keep upstream base image scripts/helpers and dbg/cli binaries (init containers rely on them),
# but replace the agent binary with seriousum build.
COPY target/release/cilium /usr/bin/cilium-agent
# Keep upstream /opt/cni/bin/cilium-cni from the base image for host glibc compatibility.
