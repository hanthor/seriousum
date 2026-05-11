# Cilium integration images

Start with `scripts/run-cilium-kind-test.sh` for the first real harness pass. It will build the Rust-backed images, optionally load them into kind, export the image overrides, and run a focused ginkgo harness invocation.

Use the image tags emitted by your build/publish step, and keep the same tag across the Cilium components.

Typical overrides:

```bash
export TAG=<published-tag>
export REGISTRY=<your-registry>

export CILIUM_IMAGE="$REGISTRY/seriousum/cilium:$TAG"
export CILIUM_OPERATOR_IMAGE="$REGISTRY/seriousum/operator:$TAG"
export HUBBLE_RELAY_IMAGE="$REGISTRY/seriousum/hubble:$TAG"
export CLUSTERMESH_APISERVER_IMAGE="$REGISTRY/seriousum/clustermesh-apiserver:$TAG"
```

If the harness also pulls other component images, point those overrides at the same registry/tag family as well (for example hubble-ui, clustermesh-apiserver, or etcd if your setup uses them).

First smoke checks after the images are built:

```bash
cilium-cli version
cilium-cli config check --path /path/to/seriousum.json
cilium-cli operator report
hubble
clustermesh-apiserver
```

Once `cilium-cli features status` and `cilium-cli sysdump` land, add them here too; the harness resolves them through the drop-in `PATH` just like the other `cilium-cli` entry points.

Then run the install/status path your harness already uses, with the image overrides exported above.

For parallel focused runs, use `scripts/run-cilium-kind-matrix.sh` and give each cluster its own focus string.

Suggested focused pass order:

1. operator health
2. operator metrics
3. controlplane
4. k8s services
5. hubble
6. fqdn
7. lrp

Keep each pass narrow, capture logs/output, and only widen scope after the focused pass is green.
