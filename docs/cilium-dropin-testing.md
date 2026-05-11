# Cilium drop-in testing

Use the helper script to build the Rust binaries and create Cilium-style aliases:

```bash
./scripts/build-cilium-dropin.sh
# or choose a different alias dir
./scripts/build-cilium-dropin.sh /tmp/cilium-dropin
```

What it does:

- runs `cargo build --release --workspace --bins`
- creates symlinks in `target/cilium-dropin/` by default
- aliases:
  - `cilium` -> `seriousum-daemon`
  - `cilium-dbg` -> `seriousum-daemon`
  - `cilium-cli` -> `seriousum-cli`
  - `operator` -> `seriousum-operator`
  - `hubble` -> `seriousum-hubble`
  - `clustermesh-apiserver` -> `seriousum-clustermesh`

Run the harness against the aliases by putting that directory on `PATH` so `cilium`, `cilium-cli`, and any future `cilium-cli` subcommands resolve to the drop-in binaries:

```bash
export PATH="$PWD/target/cilium-dropin:$PATH"
```

Most useful next checks:

```bash
cilium --config /path/to/seriousum.json
cilium-cli version
cilium-cli config check --path /path/to/seriousum.json
operator --summary "operator scaffold ready"
hubble
clustermesh-apiserver
```

When implemented, `cilium-cli features status` and `cilium-cli sysdump` should be added to this list as PATH-based smoke checks.

If you need the raw binaries, they stay in `target/release/`.
