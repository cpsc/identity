# Changelog

## v0.0.1

First alpha for evaluating passkey-based access to self-managed Kubernetes clusters.
Use a disposable cluster and keep an independent cluster-admin kubeconfig. This is not a
production-readiness or OpenID Foundation certification claim.

### Included

- Passkey enrollment and sign-in, with optional linked GitHub and Google accounts for everyday access.
- Short-lived Kubernetes access tokens; administrative access requires a fresh passkey sign-in
  and expires after at most five minutes by default, without refresh.
- Kubernetes-backed users, credentials, invitations and grants; no separate database.
- Static Linux amd64 and arm64 CLI archives and a multi-platform server image, with signed
  checksums, image signatures, CycloneDX SBOMs and source/build provenance.

### Installation and platform scope

Follow [Get started](https://github.com/zeabur-oss/identity/blob/v0.0.1/docs/get-started.md)
for installation and signature verification. Pin the image by the digest in the release's
`image-digests.txt` asset.

| Surface | v0.0.1 scope |
|---|---|
| CLI archives and server images | Linux amd64 and arm64 |
| Installation guide | Ubuntu 24.04 or later; k3s v1.36.5+k3s1 |
| Automated cluster integration and E2E | k3s v1.36.5-k3s1 |
| k0s and kubeadm | Configuration generation is implemented; clean release installation is unverified |
| macOS, Windows and other CLI targets | No prebuilt archives; source builds are outside the release artifact matrix |

CI coverage does not establish successful installation of these release artifacts. Before
making the draft public, maintainers must record clean-install, browser sign-in, permission,
expiry and account-recovery results using the
[release installation checklist](https://github.com/zeabur-oss/identity/blob/v0.0.1/docs/release-installation.md)
in [issue #43](https://github.com/zeabur-oss/identity/issues/43).

### Known limitations and security boundaries

- Alpha releases can break configuration and resource schemas. There are no migration, upgrade
  or rollback guarantees for the `v1alpha1` Kubernetes resources.
- No browser SSO sessions, RP-Initiated Logout or Back-Channel Logout. Each authorization request
  requires sign-in.
- DPoP is not implemented; kube-apiserver accepts bearer tokens. Zeabur Identity is not OpenID Foundation
  certified, and the certification suite has not been run.
- Disabling a user, resetting passkeys or revoking a grant prevents session renewal; an already
  issued access token remains usable until expiry, up to ten minutes with default settings.
- Keep SSH/console access, an independent cluster-admin credential and tested backups. See
  [operations](https://github.com/zeabur-oss/identity/blob/v0.0.1/docs/operations.md).
- Only the latest published alpha is eligible for security fixes; older alphas receive no
  backports. See the [security policy](https://github.com/zeabur-oss/identity/blob/v0.0.1/SECURITY.md).
