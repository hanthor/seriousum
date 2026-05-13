FROM gcr.io/distroless/cc-debian12

LABEL org.opencontainers.image.title="Seriousum Operator"
LABEL org.opencontainers.image.version="v0.1.0-alpha"
LABEL org.opencontainers.image.source="https://github.com/hanthor/seriousum"

# Binary must match the name the Helm chart invokes: cilium-operator-<cloud>
# For the generic cloud provider (default) that is cilium-operator-generic.
COPY seriousum-operator /usr/bin/cilium-operator-generic

ENV SERIOUSUM_VERSION="v0.1.0-alpha"
ENTRYPOINT ["/usr/bin/cilium-operator-generic"]
