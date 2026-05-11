FROM gcr.io/distroless/cc-debian12
COPY cilium-cli /usr/local/bin/cilium-cli
ENTRYPOINT ["/usr/local/bin/cilium-cli"]
