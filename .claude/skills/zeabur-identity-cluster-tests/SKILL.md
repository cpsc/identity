---
name: zeabur-identity-cluster-tests
description: Run Zeabur Identity's Kubernetes integration and end-to-end tests on a disposable k3s cluster, and debug kube-apiserver's authentication configuration. Use when changing anything in zeabur-identity-k8s, the server's background tasks, manifests, kube-config output or the container image.
---

# Cluster tests

The cluster tests are `#[ignore]`d and need a disposable cluster where you are cluster admin.
Run Docker/k3d on a Linux machine or VM (for example over SSH), not on a workstation you care
about; sync the working tree there with `rsync -a --delete --exclude target ./ host:zeabur-identity/` and
run the commands below inside `nix develop` on that machine.

## Cluster with a writable authentication config
kube-apiserver reads its `AuthenticationConfiguration` from a file and re-reads it about once a
minute, so mount a host *directory* (atomic renames inside it are seen) and start with no issuer:

```sh
mkdir -p ~/zeabur-identity-authn
cat > ~/zeabur-identity-authn/authn.yaml <<'EOF'
apiVersion: apiserver.config.k8s.io/v1
kind: AuthenticationConfiguration
anonymous:
  enabled: false
jwt: []
EOF
k3d cluster create zeabur-identity --image rancher/k3s:v1.36.5-k3s1 --no-lb --wait \
  --volume "$HOME/zeabur-identity-authn:/etc/rancher/k3s/zeabur-identity@server:0" \
  --k3s-arg "--kube-apiserver-arg=authentication-config=/etc/rancher/k3s/zeabur-identity/authn.yaml@server:0"
```
The `anonymous` section cannot change on reload; keep it as above for k3s.

## Run
```sh
cargo nextest run --locked --run-ignored only -E 'binary(kubernetes)'          # storage, RBAC, admission
./scripts/build-image.sh zeabur-identity:e2e && k3d image import zeabur-identity:e2e -c zeabur-identity
ZEABUR_IDENTITY_E2E_IMAGE=zeabur-identity:e2e ZEABUR_IDENTITY_E2E_AUTHN_DIR=$HOME/zeabur-identity-authn \
ZEABUR_IDENTITY_E2E_MEASUREMENTS=/tmp/m.json \
  cargo nextest run --locked --run-ignored only -E 'binary(e2e)'
```
The e2e test takes 1–3 minutes, mostly waiting for kube-apiserver to reload the config.

## What to check when something fails
- **Tokens never accepted** (whoami times out): read the apiserver log —
  `docker logs k3d-zeabur-identity-server-0 2>&1 | grep -iE "authentication config|oidc"`. Config
  validation errors (for example CEL type errors; JSON claims are untyped, use `dyn(...)` for
  comprehensions) make the reload skip the file permanently until it changes. A healthy reload
  logs `reloaded authentication config`.
- **Probe CEL quickly**: write a candidate config with a dummy issuer into the mounted
  directory, wait ~70 s, and read the log; a health error about the dummy issuer means the
  config itself validated. Restore the empty config afterwards.
- **`cannot change roleRef`**: a ClusterRoleBinding from an older install has a different role;
  delete it (`kubectl delete clusterrolebinding <name>`) and apply again.
- **NodePort refused**: kube-proxy needs a moment after a Service is created; poll.
- **Concurrent deletes** on k3s can return 500 for the loser; code fails closed, tests accept it.
- Measure memory with the kubelet summary API (`/api/v1/nodes/<node>/proxy/stats/summary`); the
  e2e test writes it to `ZEABUR_IDENTITY_E2E_MEASUREMENTS`.

Clean up with `k3d cluster delete zeabur-identity`.
