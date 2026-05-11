# Cilium kind matrix runner

Use `scripts/run-cilium-kind-matrix.sh` to run focused Cilium integration suites in parallel across separate kind clusters.

Example:

```bash
./scripts/run-cilium-kind-matrix.sh \
  --cluster kind-a:K8sDatapathServicesTest \
  --cluster kind-b:K8sHubbleTest \
  --cluster kind-c:K8sFQDNTest
```

What it does:

- prebuilds the Rust-backed images and shared host aliases once
- launches one `scripts/run-cilium-kind-test.sh` process per cluster/focus pair with the shared drop-in `BIN_DIR` so `PATH`-based helpers stay consistent
- passes through a per-run `--test-timeout` so hung focus groups fail instead of running forever
- stores a per-run log at `target/cilium-kind-matrix/<cluster>/run.log`
- supports a comma-separated `--matrix kind-a:...,kind-b:...` form too

Recommended use:

- keep clusters separate when the suite touches kube-system, CRDs, or shared Helm releases
- use one focus group per cluster so failures stay actionable
- reuse the same image prefix/tag family as the drop-in and image-packaging scripts
