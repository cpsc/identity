//! Proof Key for Code Exchange (RFC 7636), S256 only.
//!
//! PKCE is required for every client, not only public ones (RFC 9700 §2.1.1 requires it for
//! public clients and asks servers to prevent downgrade). A token request without a verifier is
//! refused, and `plain` is never accepted, as in OAuth 2.1. The comparison is constant-time.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use genkan_core::crypto::{ct_eq, sha256};

/// Whether `value` is 43 to 128 characters from the unreserved set (RFC 7636 §4.1 for the
/// verifier; §4.2 gives the same form for an S256 challenge, which is always 43 characters).
#[must_use]
pub fn well_formed(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// Whether `challenge` is a syntactically valid S256 challenge: 43 base64url characters.
#[must_use]
pub fn valid_s256_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && URL_SAFE_NO_PAD
            .decode(challenge)
            .is_ok_and(|d| d.len() == 32)
}

/// Checks `verifier` against `challenge` (RFC 7636 §4.6) in constant time.
#[must_use]
pub fn verify(verifier: &str, challenge: &str) -> bool {
    well_formed(verifier)
        && ct_eq(
            URL_SAFE_NO_PAD
                .encode(sha256(verifier.as_bytes()))
                .as_bytes(),
            challenge.as_bytes(),
        )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// RFC 7636 Appendix B.
    #[test]
    fn rfc7636_appendix_b() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(valid_s256_challenge(challenge));
        assert!(verify(verifier, challenge));
        assert!(!verify(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl",
            challenge
        ));
    }

    #[test]
    fn rejects_malformed_verifiers() {
        assert!(!well_formed(&"a".repeat(42)));
        assert!(well_formed(&"a".repeat(43)));
        assert!(well_formed(&"a".repeat(128)));
        assert!(!well_formed(&"a".repeat(129)));
        assert!(!well_formed(&format!("{}+", "a".repeat(43))));
        assert!(!valid_s256_challenge("plain"));
    }

    proptest! {
        #[test]
        fn only_the_right_verifier_matches(verifier in "[A-Za-z0-9._~-]{43,128}", other in "[A-Za-z0-9._~-]{43,128}") {
            let challenge = URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()));
            prop_assert!(valid_s256_challenge(&challenge));
            prop_assert!(verify(&verifier, &challenge));
            prop_assert_eq!(verify(&other, &challenge), other == verifier);
        }

        #[test]
        fn verify_never_panics(verifier in "\\PC{0,200}", challenge in "\\PC{0,200}") {
            // Only a well-formed verifier can ever match.
            if verify(&verifier, &challenge) {
                prop_assert!(well_formed(&verifier));
            }
        }
    }
}
