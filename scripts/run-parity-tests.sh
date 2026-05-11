#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

packages=(
  seriousum-core
  seriousum-config
  seriousum-crypto
  seriousum-kvstore
  seriousum-api
  seriousum-daemon
  seriousum-operator
  seriousum-cli
  seriousum-hubble
  seriousum-clustermesh
  seriousum-auth
  seriousum-proxy
  seriousum-wireguard
  seriousum-cni
  seriousum-bgp
  seriousum-fqdn
  seriousum-envoy
  seriousum-k8s
  seriousum-datapath
  seriousum-ebpf
  seriousum-controller
)

for package in "${packages[@]}"; do
  echo "==> cargo test -p ${package}"
  cargo test -p "${package}"
done
