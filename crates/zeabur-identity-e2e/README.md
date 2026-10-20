# zeabur-identity-e2e

End-to-end test of Zeabur Identity on a real Kubernetes cluster. The crate has no library code; the
scenario is `tests/e2e.rs`:

1. Install the CRDs and everything `zeabur-identityctl manifests` prints, with the image under test.
2. Prepare kube-apiserver exactly as `zeabur-identityctl kube-config --distro k3s` does (internal
   discovery through the Service's ClusterIP and a freshly issued certificate) and write the
   `AuthenticationConfiguration` into kube-apiserver's configuration directory.
3. Issue a setup link, enrol a software passkey, and sign in as kubectl's OIDC plugin would
   (authorization code with PKCE).
4. Check with `kubectl auth whoami` and `kubectl auth can-i` that kube-apiserver accepts the
   access token, that a normal token can read but not administer, that an `elevate` sign-in
   yields a short token without a refresh token in which the user is `zeabur:elevated:admins`,
   and that an ID token is rejected.
5. Check that the pod's working set after the sign-in is at most 30 MiB.

## Running it

The test is `#[ignore]`d by default. Follow
[CONTRIBUTING: Disposable k3d cluster](../../CONTRIBUTING.md#disposable-k3d-cluster) for the
canonical cluster creation, image build/import and test commands. Run them from the repository
root on a Linux machine or VM inside `nix develop`.

The scenario needs:

- `KUBECONFIG` pointing at the disposable k3s cluster as cluster admin;
- kube-apiserver reading its `AuthenticationConfiguration` from the mounted host directory;
- `ZEABUR_IDENTITY_E2E_IMAGE` naming the image already imported into that cluster;
- `ZEABUR_IDENTITY_E2E_AUTHN_DIR` naming the same host directory (CI uses `$RUNNER_TEMP/authn`);
- optionally, `ZEABUR_IDENTITY_E2E_MEASUREMENTS` naming a file to receive the measured working set.

The `kubernetes` job in `.github/workflows/ci.yml` runs this setup on every change.
kube-apiserver re-reads the file about once a minute and then fetches discovery, so the test
waits up to three minutes for it.

## Place in the workspace

Uses `zeabur-identity-core`, `zeabur-identity-k8s`, `zeabur-identity-oauth`, `zeabur-identity-signin` and `zeabur-identity-test-support` as
dev-dependencies. Nothing depends on it.
