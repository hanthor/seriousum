FROM gcr.io/distroless/cc-debian12
COPY hubble /usr/local/bin/hubble
COPY hubble-relay /usr/local/bin/hubble-relay
ENTRYPOINT ["/usr/local/bin/hubble"]
