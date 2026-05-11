FROM gcr.io/distroless/cc-debian12
COPY cilium-dbg /usr/local/bin/cilium-dbg
ENTRYPOINT ["/usr/local/bin/cilium-dbg"]
