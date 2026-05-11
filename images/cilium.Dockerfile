FROM gcr.io/distroless/cc-debian12
COPY cilium /usr/local/bin/cilium
COPY cilium-dbg /usr/local/bin/cilium-dbg
ENTRYPOINT ["/usr/local/bin/cilium"]
