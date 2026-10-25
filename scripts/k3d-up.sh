#!/usr/bin/env bash
# Creates a disposable k3d cluster for the cluster and end-to-end tests. kube-apiserver reads its
# AuthenticationConfiguration from <dir>/authn/authn.yaml, which starts with no issuer; the
# end-to-end test replaces it (ZEABUR_IDENTITY_E2E_AUTHN_DIR=<dir>/authn). The kubeconfig goes to
# <dir>/kubeconfig, never into the default one. Run inside `nix develop` on Linux with Docker.
#   scripts/k3d-up.sh <dir> [k3d cluster create arguments...]
#   export KUBECONFIG=<dir>/kubeconfig
#
# CLUSTER names the cluster (default zeabur-identity); K3S_IMAGE picks the k3s release.
# ZEABUR_REGISTRY_MIRRORS (`docker.io=http://mirror:5001,ghcr.io=...`) makes the node pull
# through those mirrors. Delete the cluster with `k3d cluster delete <name>`.
set -euo pipefail

dir="${1:?usage: k3d-up.sh <dir> [k3d cluster create arguments...]}"
shift
mkdir -p "$dir/authn"
dir="$(cd "$dir" && pwd)"
cluster="${CLUSTER:-zeabur-identity}"

# Mount the directory, not the file: kube-apiserver must see the atomic replacement. The
# `anonymous` section cannot change on a reload.
printf 'apiVersion: apiserver.config.k8s.io/v1\nkind: AuthenticationConfiguration\nanonymous:\n  enabled: false\njwt: []\n' \
  > "$dir/authn/authn.yaml"

args=()
if [[ -n "${ZEABUR_REGISTRY_MIRRORS:-}" ]]; then
  IFS=, read -ra mirrors <<< "$ZEABUR_REGISTRY_MIRRORS"
  {
    echo "mirrors:"
    for mirror in "${mirrors[@]}"; do
      [[ "$mirror" == ?*=?* ]] || { echo "error: ZEABUR_REGISTRY_MIRRORS: want registry=endpoint, got $mirror" >&2; exit 1; }
      printf '  "%s":\n    endpoint: ["%s"]\n' "${mirror%%=*}" "${mirror#*=}"
    done
  } > "$dir/registries.yaml"
  args+=(--registry-config "$dir/registries.yaml")
fi

k3d cluster create "$cluster" --image "${K3S_IMAGE:-rancher/k3s:v1.36.5-k3s1}" --no-lb --wait --timeout 300s \
  --kubeconfig-update-default=false --kubeconfig-switch-context=false "${args[@]}" \
  --volume "$dir/authn:/etc/rancher/k3s/zeabur-identity@server:0" \
  --k3s-arg "--kube-apiserver-arg=authentication-config=/etc/rancher/k3s/zeabur-identity/authn.yaml@server:0" \
  "$@"
k3d kubeconfig get "$cluster" > "$dir/kubeconfig"
