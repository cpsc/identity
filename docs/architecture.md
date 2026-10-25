# Architecture

Zeabur Identity is a passkey-first OpenID Connect issuer for
self-managed Kubernetes clusters. k3s is tested in CI; RKE2 uses the same configuration but has
no release-installation verification. kube-apiserver verifies Zeabur Identity's tokens
itself through [Structured Authentication Configuration][kep-3331], so people sign in with a
passkey instead of carrying a long-lived admin kubeconfig that cannot be revoked. Any OpenID
Connect client (Argo CD, Grafana, Headlamp, …) can use the same issuer.

## Security properties

- **No passwords.** Passkeys (WebAuthn, user verification required) are the primary credential;
  GitHub and Google accounts can be linked as alternatives for ordinary access.
- **Admin rights belong to admin accounts.** Only a `User` with `accountType: Admin` gets the
  `zeabur:admins` group, bound to `cluster-admin`. An admin account signs in with a passkey
  only, its tokens last five minutes by default, and its sessions can idle out sooner. With
  the install-wide AAL3 setting it also needs a device-bound passkey. Regular accounts get only
  the rights bound to their groups. Every session ends 12 hours after sign-in, and
  kube-apiserver's configuration bounds every token's lifetime independently.
- **No standing secrets in the issuer's hands.** Tokens are short-lived and audience-bound; only
  hashes of codes, refresh tokens and links are stored; signing keys rotate on a schedule. The
  service account cannot create Secrets, bindings or users.
- **Self-service without new privileges.** The account page acts only on the signed-in user's
  own objects, matched by UID; adding or removing a passkey needs a fresh verified passkey, and
  the last one cannot be removed.
- **State in Kubernetes.** Custom resources hold users, credentials, clients, sessions and
  invitations, with no database; see [removing someone](managing-users.md#suspend-or-remove-someone)
  for revocation and deletion order.
- **Small.** One static binary in an otherwise empty image (about 20 MB), a few MiB of memory at
  rest; sign-in pages are server-rendered with about 1 KB of script each.

[conformance.md](conformance.md) maps every specification requirement Zeabur Identity implements to the
code and the test that proves it.

## How it fits together

```text
browser ──passkey──▶ zeabur-identity (in the cluster) ──tokens──▶ kubectl / OIDC clients
                        │  ▲                                     │
                 custom resources                       bearer token
                        ▼  │                                     ▼
                  kube-apiserver ◀──discovery + keys (internal TLS)── verifies tokens itself
```

A `kubectl` request goes like this:

1. kubelogin opens the browser at Zeabur Identity's sign-in page.
2. The person signs in with a passkey. kubelogin receives a short-lived access token and a
   refresh token, and Zeabur Identity records the session as a `Grant` object.
3. `kubectl` sends the token to kube-apiserver.
4. kube-apiserver checks the token's signature against Zeabur Identity's published keys, which it
   fetches over an internal TLS connection. Zeabur Identity is not involved in the request itself.
5. When the token expires, kubelogin renews it with a refresh token, without the browser.

The admin tool `zeabur-identityctl` runs on an administrator's machine with their own kubeconfig. It
prints the install manifests, writes kube-apiserver's `AuthenticationConfiguration` for the
distribution, and issues the one-time links people use to enrol a passkey.

`deploy/crds.json` and `deploy/rbac.json` are the committed outputs of `zeabur-identityctl crds` and
`zeabur-identityctl rbac`, for review in pull requests.

## Crates

The code is split into small crates in one flat `crates/` directory:

- **Dependencies only point downwards**, with no cycles ([Kladov][matklad-workspaces]; Lakos and
  Martin, see [References](#references)).
- **Each crate hides one design decision** behind a narrow interface ([Parnas][parnas]).
- **Storage is a set of traits** with an in-memory and a Kubernetes implementation (ports and
  adapters, [Cockburn][cockburn]), so most tests need no cluster.
- **The server and the admin CLI are separate binaries**, as in Kanidm (`kanidmd`/`kanidm`),
  etcd (`etcd`/`etcdctl`) and cert-manager (`cmctl`). Code only an administrator should run
  never ships in the server image.

Every crate has a README that is also its crate-level documentation (`cargo doc --open`).

| Crate | What it holds | Depends on |
|---|---|---|
| [`zeabur-identity-core`](../crates/zeabur-identity-core) | time, cryptographic helpers, validated config values, protocol names | — |
| [`zeabur-identity-keys`](../crates/zeabur-identity-keys) | signing keys, JWK publication, JWS signing and verification, rotation | core |
| [`zeabur-identity-store`](../crates/zeabur-identity-store) | storage ports (traits, records) and in-memory adapters | core, keys |
| [`zeabur-identity-tls`](../crates/zeabur-identity-tls) | trust roots, certificate issuance, a reloadable TLS listener | core |
| [`zeabur-identity-ui`](../crates/zeabur-identity-ui) | server-rendered pages, embedded assets, translations, security headers | core |
| [`zeabur-identity-oauth`](../crates/zeabur-identity-oauth) | the OAuth 2.0 / OpenID Connect authorization server | core, keys, store, ui |
| [`zeabur-identity-signin`](../crates/zeabur-identity-signin) | passkey enrolment and sign-in, upstream providers, page routes | core, keys, store, oauth, ui |
| [`zeabur-identity-k8s`](../crates/zeabur-identity-k8s) | custom resources, Kubernetes adapters, GC, manifests, kube-apiserver config | core, keys, store, tls |
| [`zeabur-identity`](../crates/zeabur-identity) | **binary**: the in-cluster server | all of the above |
| [`zeabur-identityctl`](../crates/zeabur-identityctl) | **binary**: the administrator's CLI | core, k8s |
| [`zeabur-identity-test-support`](../crates/zeabur-identity-test-support), [`zeabur-identity-e2e`](../crates/zeabur-identity-e2e) | test helpers; end-to-end tests on a cluster | — |

The sign-in pages' TypeScript and Tailwind sources live in [`ui/`](../ui), with Zeabur's design
system as a package. Their built output, and the design system's classes as askama macros
(`templates/ds.html`), are committed to `crates/zeabur-identity-ui` and embedded in the binary, so
building Zeabur Identity needs only Cargo.

## Development

See [CONTRIBUTING.md](../CONTRIBUTING.md) for setup, checks, dependency policy and cluster tests.

## References

- The Kubernetes Authors, "KEP-3331: Structured Authentication Configuration."
  <https://github.com/kubernetes/enhancements/tree/master/keps/sig-auth/3331-structured-authentication-configuration>
- A. Kladov, "Large Rust Workspaces," Aug. 22, 2021.
  <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
- D. L. Parnas, "On the criteria to be used in decomposing systems into modules," *Commun.
  ACM*, vol. 15, no. 12, pp. 1053–1058, Dec. 1972, doi:
  [10.1145/361598.361623](https://doi.org/10.1145/361598.361623).
- J. Lakos, *Large-Scale C++ Software Design*. Addison-Wesley, 1996.
- R. C. Martin, *Agile Software Development, Principles, Patterns, and Practices*. Prentice
  Hall, 2002 (the Acyclic and Stable Dependencies Principles).
- A. Cockburn, "The Hexagonal (Ports & Adapters) Architecture," 2005.
  <https://alistair.cockburn.us/hexagonal-architecture/>

[kep-3331]: https://github.com/kubernetes/enhancements/tree/master/keps/sig-auth/3331-structured-authentication-configuration
[matklad-workspaces]: https://matklad.github.io/2021/08/22/large-rust-workspaces.html
[parnas]: https://doi.org/10.1145/361598.361623
[cockburn]: https://alistair.cockburn.us/hexagonal-architecture/
