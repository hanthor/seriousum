FROM gcr.io/distroless/cc-debian12
COPY clustermesh-apiserver /usr/local/bin/clustermesh-apiserver
ENTRYPOINT ["/usr/local/bin/clustermesh-apiserver"]
