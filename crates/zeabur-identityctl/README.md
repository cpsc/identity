# zeabur-identityctl

The Zeabur Identity admin CLI. It runs on an administrator's machine with the administrator's own
kubeconfig, never inside the cluster, and is not part of the server image. Work that needs more
rights than Zeabur Identity's service account has (installing RBAC, creating users and the setup
Secret, having cert-manager issue the internal certificate) therefore happens here, with a
human's credentials.

## Commands

Set the public issuer URL before running commands that need it:

```sh
export ZEABUR_IDENTITY_ISSUER=https://auth.example.com
```

Replace the example URL with your own issuer. `zeabur-identityctl` reads `ZEABUR_IDENTITY_ISSUER` for
`manifests`, `kube-config`, `setup-link` and `invite`, so those commands need no issuer flag.

- `zeabur-identityctl crds`: the custom resource definitions as a Kubernetes `List`, for
  `kubectl apply --server-side -f -`.
- `zeabur-identityctl rbac`: the namespace, service account, `Role`, `RoleBinding`, the admin bindings and
  the empty key Secret.
- `zeabur-identityctl manifests --image <ref>`: everything `rbac` prints plus the Service
  and the single-replica Deployment.
- `zeabur-identityctl kube-config --distro <k3s|generic>`: issues the
  internal listener's certificate if needed (the CA key is discarded), prints the kube-apiserver
  `AuthenticationConfiguration` on stdout and how to install it on stderr. `k3s` covers K3s/RKE2.
- `zeabur-identityctl setup-link [--username admin] [--reset]`: a one-time link that
  enrols a passkey for the first admin (group `admins`, elevated group `admins`), or recovers
  the account with `--reset`, which first deletes the user's pending invitations, passkeys and
  sessions. It expires after 60 minutes by default.
- `zeabur-identityctl invite --username <name>`: creates the `User` if needed and a
  one-time enrolment link (7 days by default), printed for you to send.
- `zeabur-identityctl client create <client-id> --type <public|confidential> --redirect-uri <uri>`:
  prints an `OAuthClient` after checking it against the same rules as the CRD schema. A
  confidential client gets a generated 256-bit secret, printed once on stderr; the object holds
  only its hash. See [Registering clients](../../docs/clients.md).

Results (JSON, YAML or a link) go to stdout and messages to stderr, so commands compose with
`kubectl apply -f -`.

### Invite users

The server must accept invitations: enable `zeabur-identity --invitations` or set
`ZEABUR_IDENTITY_INVITATIONS=true` in its Deployment. When generating the Deployment, pass
`--serve-arg=--invitations` to `zeabur-identityctl manifests`.

```sh
zeabur-identityctl invite --username alice --display-name 'Alice' \
  --group developers --elevated-group operators --valid-for 1d
```

`--group` and `--elevated-group` are repeatable. Together with `--display-name`, they apply
**only when creating a new user**; inviting an existing user does not change their name or
groups. Edit that user's `User` resource to change those fields. Ordinary groups appear to
Kubernetes as `zeabur:<group>`; elevated groups appear as `zeabur:elevated:<group>` only
after a passkey sign-in requesting the `elevate` scope. `--valid-for` controls how long the
link can be redeemed (default `7d`). Send the printed link privately.

### Configure token limits and the internal certificate

`kube-config` generates a maximum accepted token lifetime of `1h` and an elevated-token
maximum of `5m`. If the server's `--elevated-token-lifetime` is raised above its `5m` default,
set `--max-elevated-token-lifetime` to match when generating the configuration, then install
the updated configuration on kube-apiserver. For example, for a server issuing 10-minute
elevated tokens:

```sh
zeabur-identityctl kube-config --distro k3s --max-elevated-token-lifetime 10m
```

The default internal discovery mode needs [cert-manager](https://cert-manager.io/docs/).
`kube-config` applies a root CA (10 years), a `CA` Issuer and a serving certificate in Zeabur
Identity's namespace, waits until cert-manager issues them, and prints the root CA in the
configuration. cert-manager renews the serving certificate with its defaults (90 days, renewed
30 days before expiry). The configuration does not change when it does. Without cert-manager,
`kube-config` stops with an error; use `--discovery public` instead.

Check the certificates with `kubectl -n zeabur-identity get certificate`. To change the serving
certificate's duration, set `spec.duration` and `spec.renewBefore` on Certificate
`zeabur-identity-internal-tls` with `kubectl`; `kube-config` does not reset them. See the
[deployment guide](../../docs/get-started.md) for installation steps and
[operations](../../docs/operations.md#internal-certificate) to replace the root CA.

## Place in the workspace

Depends on `zeabur-identity-core` and `zeabur-identity-k8s`. Nothing depends on it.

## Key properties

- **Least privilege by default.** The installed bindings give `zeabur:admins` the built-in
  `view` role and `cluster-admin` only to `zeabur:elevated:admins`, which appears only in short
  tokens issued right after a passkey sign-in.
- **Secrets stay hashed.** Setup and invitation tokens and client secrets are 256-bit random
  values; only their SHA-256 is written to the cluster, and a link carries its token in the URL
  fragment.
- **Single use.** Redeeming a setup link or invitation deletes its object with `uid` and
  `resourceVersion` preconditions, so only one redemption can succeed; a new setup link
  invalidates the previous one.
