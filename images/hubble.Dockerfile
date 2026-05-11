FROM gcr.io/distroless/cc-debian12
COPY hubble /usr/local/bin/hubble
ENTRYPOINT ["/usr/local/bin/hubble"]
