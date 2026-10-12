# genkan-e2e

End-to-end test of Genkan on a real Kubernetes cluster. The crate has no library code; the
scenario is `tests/e2e.rs`:

1. Install the CRDs and everything `genkanctl manifests` prints, with the image under test.
2. Prepare kube-apiserver exactly as `genkanctl kube-config --distro k3s` does (internal
   discovery through the Service's ClusterIP and a freshly issued certificate) and write the
   `AuthenticationConfiguration` into kube-apiserver's configuration directory.
3. Issue a setup link, enrol a software passkey, and sign in as kubectl's OIDC plugin would
   (authorization code with PKCE).
4. Check with `kubectl auth whoami` and `kubectl auth can-i` that kube-apiserver accepts the
   access token, that a normal token can read but not administer, that an `elevate` sign-in
   yields a short token without a refresh token in which the user is `genkan:elevated:admins`,
   and that an ID token is rejected.
5. Check that the pod's working set after the sign-in is at most 30 MiB.

## Running it

The test is `#[ignore]`d by default. It needs:

- a k3s cluster whose kube-apiserver reads its `AuthenticationConfiguration` from a directory on
  the machine running the test, for example with k3d:

  ```sh
  mkdir -p "$PWD/authn"
  printf 'apiVersion: apiserver.config.k8s.io/v1\nkind: AuthenticationConfiguration\nanonymous:\n  enabled: false\njwt: []\n' \
    > "$PWD/authn/authn.yaml"
  k3d cluster create genkan --no-lb --wait \
    --volume "$PWD/authn:/etc/rancher/k3s/genkan@server:0" \
    --k3s-arg "--kube-apiserver-arg=authentication-config=/etc/rancher/k3s/genkan/authn.yaml@server:0"
  ```

- the image loaded into the cluster (`scripts/build-image.sh genkan:e2e`, then
  `k3d image import genkan:e2e --cluster genkan`);
- `KUBECONFIG` pointing at the cluster as cluster admin, `GENKAN_E2E_IMAGE` naming the image,
  `GENKAN_E2E_AUTHN_DIR` naming the host directory above, and optionally
  `GENKAN_E2E_MEASUREMENTS`, a file the measured working set is written to.

Then run `cargo nextest run --run-ignored only -E 'binary(e2e)'`. The `kubernetes` job in
`.github/workflows/ci.yml` does all of this on every change. kube-apiserver re-reads the file
about once a minute and then fetches discovery, so the test waits up to three minutes for it.

## Place in the workspace

Uses `genkan-core`, `genkan-k8s`, `genkan-oauth`, `genkan-signin` and `genkan-test-support` as
dev-dependencies. Nothing depends on it.
