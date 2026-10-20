#![doc = include_str!("../README.md")]

pub mod jwk;
pub mod jws;
pub mod manager;
pub mod rotation;
pub mod signing;
pub mod store;
pub mod verify;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

/// A JWS signing algorithm this issuer uses.
///
/// kube-apiserver accepts only the RS, ES and PS families ([kube-apiserver OIDC]), so EdDSA is
/// not offered. ES256 is the default for every token: smaller tokens and faster signing than
/// RS256, and [RFC 7518] §3.1 marks it "Recommended+". RS256 exists only because
/// [OIDC Discovery] §3 requires it in `id_token_signing_alg_values_supported`.
///
/// [kube-apiserver OIDC]: https://github.com/kubernetes/kubernetes/blob/master/staging/src/k8s.io/apiserver/plugin/pkg/authenticator/token/oidc/oidc.go
/// [RFC 7518]: https://www.rfc-editor.org/rfc/rfc7518
/// [OIDC Discovery]: https://openid.net/specs/openid-connect-discovery-1_0.html
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub enum Algorithm {
    /// ECDSA using P-256 and SHA-256 (RFC 7518 §3.4).
    #[serde(rename = "ES256")]
    Es256,
    /// RSASSA-PKCS1-v1_5 using SHA-256 (RFC 7518 §3.3).
    #[serde(rename = "RS256")]
    Rs256,
}

impl Algorithm {
    /// All algorithms, in the order they are advertised.
    pub const ALL: [Self; 2] = [Self::Es256, Self::Rs256];

    /// The JWA name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Es256 => "ES256",
            Self::Rs256 => "RS256",
        }
    }
}

/// Encodes bytes as unpadded base64url (RFC 7515 §2).
#[must_use]
pub fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}
