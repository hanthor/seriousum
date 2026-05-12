# Seriousum Documentation

Current, maintained documentation for Seriousum.

This directory is intentionally limited to docs that are still current. Historical planning/spec material has been moved to `docs/archive/`.

---

## Start here

### For most readers
1. [README.md](../README.md)
2. [PARITY_PROOF_DASHBOARD.md](PARITY_PROOF_DASHBOARD.md)
3. [INSTALLATION.md](INSTALLATION.md)
4. [TROUBLESHOOTING.md](TROUBLESHOOTING.md)

---

## Active documentation set

### User / operator docs
- [INSTALLATION.md](INSTALLATION.md) — installation and deployment methods
- [TROUBLESHOOTING.md](TROUBLESHOOTING.md) — debugging and operational help
- [ARCHITECTURE.md](ARCHITECTURE.md) — system design and component overview

### Parity / evidence docs
- [PARITY_PROOF_DASHBOARD.md](PARITY_PROOF_DASHBOARD.md) — authoritative parity-proof status
- [component-porting-compliance.md](component-porting-compliance.md) — crate/component evidence report
- [parity-matrix.md](parity-matrix.md) — Rust ↔ Cilium mapping
- [FULL_TEST_SUITE_CATALOG.md](FULL_TEST_SUITE_CATALOG.md) — integration and test inventory

### Contributor docs
- [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) — contributor workflow and development setup

### Generated evidence
- [generated/BENCHMARKS.md](generated/BENCHMARKS.md) — benchmark comparison report
- [generated/benchmark-results.json](generated/benchmark-results.json) — machine-readable benchmark data
- [generated/parity-proof.json](generated/parity-proof.json) — machine-readable parity-proof status

---

## Documentation policy

A doc stays in `docs/` only if it is one of:
- a current user/operator reference
- a current contributor reference
- a current parity/evidence artifact
- a generated evidence artifact

Planning docs, implementation specs, release-time snapshots, and superseded strategies belong in `docs/archive/`.

---

## Archived material

Historical docs are in:
- `docs/archive/`

These are preserved for context, but they are not the current source of truth.

---

## Authoritative status sources

When docs disagree, trust these first:
1. [PARITY_PROOF_DASHBOARD.md](PARITY_PROOF_DASHBOARD.md)
2. [generated/parity-proof.json](generated/parity-proof.json)
3. [generated/BENCHMARKS.md](generated/BENCHMARKS.md)
4. [component-porting-compliance.md](component-porting-compliance.md)
5. [README.md](../README.md)
