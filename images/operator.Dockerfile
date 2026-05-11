FROM gcr.io/distroless/cc-debian12
COPY operator /usr/local/bin/cilium-operator-generic
ENTRYPOINT ["/usr/local/bin/cilium-operator-generic"]
