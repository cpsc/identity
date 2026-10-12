# genkan-keys

Genkan's token signing keys: generation, the rotation schedule, storage, JWK set publication,
JWS signing, and JWS verification.

kube-apiserver and every OpenID Connect client verify Genkan's tokens offline against the
published JWK set. Keys therefore have to change regularly (no long-lived keys) without a
valid token ever becoming unverifiable.

## Contents

- [`signing`](crate::signing): [`SigningKey`](crate::signing::SigningKey), an aws-lc-rs key
  bound to exactly one [`Algorithm`](crate::Algorithm) ([RFC 8725] §3.1).
- [`jwk`](crate::jwk): public JWKs ([RFC 7517]) and the [RFC 7638] thumbprint used as the `kid`.
- [`jws`](crate::jws): JWS Compact Serialization ([RFC 7515] §7.1) for tokens this issuer
  emits.
- [`verify`](crate::verify): compact JWS verification against a JWK set, for Genkan's own
  access tokens and upstream ID tokens.
- [`rotation`](crate::rotation): the schedule, as a pure function of time
  ([`KeyRing::tick`](crate::rotation::KeyRing::tick)).
- [`store`](crate::store): the versioned storage format and the
  [`KeyStore`](crate::store::KeyStore) trait (compare-and-swap).
- [`manager`](crate::manager): [`KeyManager`](crate::manager::KeyManager), which loads, rotates,
  stores and then publishes the keys.

## Place in the workspace

Depends on `genkan-core`. Used by `genkan-store`, `genkan-oauth`, `genkan-signin`,
`genkan-k8s` (which implements [`KeyStore`](crate::store::KeyStore) on a Kubernetes Secret)
and the `genkan` server.

## Key properties

- **ES256 by default, RS256 because OpenID Connect requires it.** Every token is signed with
  ES256 (P-256). An RS256 (RSA-3072) ring exists because [OIDC Discovery] §3 requires RS256 in
  `id_token_signing_alg_values_supported`. EdDSA is not offered: kube-apiserver accepts only the
  RS, ES and PS families ([kube-apiserver OIDC]).
- **Published 24 h before signing, retained 24 h after.** Each key signs for 7 days. The
  publish-ahead time must exceed every verifier's JWK set cache lifetime; the retention time
  must exceed the longest token lifetime ([OIDC Core] §10.1.1). See [`rotation`](crate::rotation).
- **Stored before used.** A key is written to storage with compare-and-swap before it signs or
  is published, and a keyset that cannot be decoded is never overwritten.
- **No algorithm confusion.** Each JWK carries `use: "sig"` and a fixed `alg`; no symmetric or
  `none` algorithm exists in the code ([RFC 7518] §3.6); JWS headers carry only `alg`, `kid` and
  `typ` ([RFC 8725] §3.10, §3.11).
- **Randomness.** Keys and ECDSA nonces come from aws-lc's system RNG. `Debug` output never
  contains key material.

A stolen private key can mint tokens until it leaves the JWK set, at most rotation period plus
retention (about 8 days with the defaults). There is no emergency revocation of a single key.

## References

- [RFC 7515] M. Jones, J. Bradley, and N. Sakimura, "JSON Web Signature (JWS)," RFC 7515, IETF,
  May 2015.
- [RFC 7517] M. Jones, "JSON Web Key (JWK)," RFC 7517, IETF, May 2015.
- [RFC 7518] M. Jones, "JSON Web Algorithms (JWA)," RFC 7518, IETF, May 2015.
- [RFC 7638] M. Jones and N. Sakimura, "JSON Web Key (JWK) Thumbprint," RFC 7638, IETF,
  Sep. 2015.
- [RFC 8725] Y. Sheffer, D. Hardt, and M. Jones, "JSON Web Token Best Current Practices,"
  RFC 8725, BCP 225, IETF, Feb. 2020.
- [OIDC Core] N. Sakimura, J. Bradley, M. Jones, B. de Medeiros, and C. Mortimore, "OpenID
  Connect Core 1.0 incorporating errata set 2," OpenID Foundation, Dec. 2023.
- [OIDC Discovery] N. Sakimura, J. Bradley, M. Jones, and E. Jay, "OpenID Connect Discovery 1.0
  incorporating errata set 2," OpenID Foundation, Dec. 2023.
- [kube-apiserver OIDC] The Kubernetes Authors,
  `staging/src/k8s.io/apiserver/plugin/pkg/authenticator/token/oidc/oidc.go`.

[RFC 7515]: https://www.rfc-editor.org/rfc/rfc7515
[RFC 7517]: https://www.rfc-editor.org/rfc/rfc7517
[RFC 7518]: https://www.rfc-editor.org/rfc/rfc7518
[RFC 7638]: https://www.rfc-editor.org/rfc/rfc7638
[RFC 8725]: https://www.rfc-editor.org/rfc/rfc8725
[OIDC Core]: https://openid.net/specs/openid-connect-core-1_0.html
[OIDC Discovery]: https://openid.net/specs/openid-connect-discovery-1_0.html
[kube-apiserver OIDC]: https://github.com/kubernetes/kubernetes/blob/master/staging/src/k8s.io/apiserver/plugin/pkg/authenticator/token/oidc/oidc.go
