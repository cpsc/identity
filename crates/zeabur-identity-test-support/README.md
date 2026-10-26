# zeabur-identity-test-support

Test helpers shared by Zeabur Identity's crates. It provides a software passkey authenticator, built on
the `passkey` crate, that stands in for the browser and the user's device in
end-to-end tests of enrolment and sign-in.

- `device` creates a `Device`: a WebAuthn client holding at most one passkey, which answers
  discoverable requests (an empty allow list).
- `VerifiedUser` is the device's user check: presence and verification (biometric or PIN)
  always pass, so the tests exercise Zeabur Identity's user-verification requirement with the UV flag set.
- `certificate::Ca` is a CA that issues serving certificates, in place of cert-manager's `CA`
  Issuer in a cluster.
- `logging::Capture` supplies an isolated native JSON tracing subscriber and parsed records.
  Attach its `Dispatch` to each complete asynchronous scenario with `with_subscriber` rather
  than setting a global subscriber. Current-span and span-list fields are disabled, matching
  the server's JSON output.

It depends on no other Zeabur Identity crate. It is a development dependency of `zeabur-identity-signin`,
`zeabur-identity-oauth`, `zeabur-identity-tls`, `zeabur-identity-k8s`, `zeabur-identity` and the
end-to-end tests, and is never part of a release build.
