# Changelog

## Unreleased

### Changed

- Internal discovery needs cert-manager. `zeabur-identityctl kube-config` has cert-manager issue
  a root CA (10 years) and the internal listener's certificate (90 days, renewed automatically)
  in Zeabur Identity's namespace. The printed configuration changes only if the root CA or the
  ClusterIP changes. Without cert-manager, use `--discovery public`.
- Upgrade: install cert-manager, then run `kube-config` once and install the configuration it
  prints on every control-plane machine. It replaces the old certificate and its CA. Sign-ins fail
  until kube-apiserver loads the new configuration, about a minute.
- The root CA's private key now lives in Secret `zeabur-identity-internal-ca`. Anyone who can read
  Secrets in Zeabur Identity's namespace can issue a certificate for the internal address.
- Admin accounts replace elevation. A `User` with `accountType: Admin` signs in with a passkey
  only, and its tokens carry `zeabur:admins`, bound to `cluster-admin` by the new
  ClusterRoleBinding `zeabur-identity-cluster-admins`. `zeabur-identityctl setup-link` creates
  the first user as an admin account. New commands: `zeabur-identityctl invite --admin` and
  `zeabur-identityctl account-type <name> <admin|regular>`.
- Sessions follow NIST SP 800-63B-4: regular accounts' tokens last 10 minutes, admin accounts'
  5 minutes; sessions end after 1 hour idle (15 minutes for admins with several teams or AAL3)
  and 12 hours after sign-in. New server options: `--refresh-idle`, `--refresh-absolute`,
  `--admin-token-lifetime`, `--admin-refresh-idle`.
- The ConfigMap `zeabur-identity-settings` holds install-wide settings: `teams`
  (`single` or `multiple`) and `admin-aal3`, which limits admin accounts to device-bound
  passkeys.

### Removed

- `zeabur-identityctl kube-config --renew-certificate` and `--certificate-validity`. cert-manager
  renews the certificate. See [operations](docs/operations.md#internal-certificate) to replace
  the root CA.
- Elevation: the `elevate` scope, `elevatedGroups` on users, `allowElevation` on clients
  (`client create --allow-elevation`), the `can_elevate` claim, `--elevated-token-lifetime`,
  `kube-config --max-elevated-token-lifetime` and `invite --elevated-group`. The read-only
  `view` binding for `zeabur:admins` (`zeabur-identity-admins`) and the elevated binding
  (`zeabur-identity-elevated-admins`) are gone. To upgrade, follow
  [From elevation to admin accounts](docs/operations.md#from-elevation-to-admin-accounts).
- `zeabur-identityctl kube-config --distro` no longer accepts `k0s`, `kubeadm` or `kubesolo`.
  Use `k3s` on K3s/RKE2 and `generic` elsewhere.
- A new passkey without a name is no longer named after its authenticator or browser. The
  account page shows it as "Passkey added" and its creation date. Existing names stay.

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
