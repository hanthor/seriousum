# Cilium kind matrix audit

## Summary
The parallel kind matrix runner already matches the harness expectations for the planned `cilium-cli` additions:

- `scripts/run-cilium-kind-matrix.sh` prebuilds the image set and the shared drop-in alias dir once, then runs each child with `BUILD_IMAGES=0`, `INSTALL_DROPIN=0`, and a shared `BIN_DIR`.
- `scripts/run-cilium-kind-test.sh` exports `PATH="$BIN_DIR:$PATH"` before invoking the Cilium harness, so new `cilium-cli` subcommands will resolve correctly from the drop-in aliases.
- The image override wiring remains consistent: the runner passes the same `IMAGE_PREFIX`/`IMAGE_TAG` family through to the child harness and the child exports the expected component image variables plus `CLUSTERMESH_INSTALL_OVERRIDES`.

## Adjustments made
Minimal doc fixes only:

- clarified that the integration helper runs a focused ginkgo harness, not a `make -C test k8s-kind` wrapper
- made the drop-in PATH requirement explicit for `cilium`, `cilium-cli`, and future `cilium-cli` subcommands
- noted that `cilium-cli features status` and `cilium-cli sysdump` should be added to the smoke-check lists once those commands land
- called out in the matrix doc that the child harnesses share the same drop-in `BIN_DIR` to preserve PATH-based resolution

## Validation
- `bash -n` passed for:
  - `scripts/run-cilium-kind-matrix.sh`
  - `scripts/run-cilium-kind-test.sh`
  - `scripts/build-cilium-dropin.sh`
  - `images/build-cilium-images.sh`
- `git diff --check` passed

## Open risks / follow-up
- When `cilium-cli features status` and `cilium-cli sysdump` are implemented, add their exact invocation forms to the smoke-check docs if they differ from the plain subcommand names.
- If the harness later requires additional component images, mirror those in both the build script and the `run-cilium-kind-test.sh` load/override section.
