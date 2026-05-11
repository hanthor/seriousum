# Cilium CLI Harness Support

Implemented support in `crates/cli/src/lib.rs` for the Cilium harness commands used by the k8s tests:

- `cilium-cli features status`
  - Added a new top-level `features` command with `status` subcommand.
  - Supports `-o markdown` and `-o json`.
  - Supports `--output-file` by writing the rendered output to disk.
  - Returns a stable, deterministic report payload.

- `cilium-cli sysdump`
  - Added a new top-level `sysdump` command.
  - Accepts `--output-filename`.
  - Writes a deterministic JSON artifact summary when a filename is provided.
  - Returns the same summary text to stdout.

Existing behaviors remain intact:
- `version`
- `config check`
- `operator report`

## Files changed
- `crates/cli/Cargo.toml`
- `crates/cli/src/lib.rs`
- `progress.md`

## Validation
- `cargo check -p seriousum-cli`
- `cargo test -p seriousum-cli`

## Tests added
- command parsing coverage for `features status` and `sysdump`
- JSON and markdown execution coverage for `features status`
- output-file coverage for `features status`
- deterministic artifact coverage for `sysdump`
