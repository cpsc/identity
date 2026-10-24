//! Authenticator names by AAGUID, for a passkey's default label.
//!
//! `assets/aaguid.json` is a snapshot of `combined_aaguid.json` from the community list
//! [passkey-authenticator-aaguids] at commit `0940476`, taken on 2026-10-20 (the file records
//! both). Only the names are kept.
//! The repository has no licence file; its README offers the list for this purpose.
//!
//! [passkey-authenticator-aaguids]: https://github.com/passkeydeveloper/passkey-authenticator-aaguids

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Data {
    authenticators: HashMap<String, String>,
}

static DATA: LazyLock<Data> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/aaguid.json")).unwrap_or_else(|error| {
        tracing::error!(%error, reason = "aaguid_data_invalid", "the AAGUID list does not parse");
        Data::default()
    })
});

/// The authenticator's name, for example "Google Password Manager".
#[must_use]
pub fn name(aaguid: &str) -> Option<&'static str> {
    DATA.authenticators
        .get(aaguid)
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_authenticators_have_names() {
        assert_eq!(
            name("fbfc3007-154e-4ecc-8c0b-6e020557d7bd"),
            Some("Apple Passwords")
        );
        assert_eq!(
            name("ea9b8d66-4d01-1d21-3ce4-b6b48cb575d4"),
            Some("Google Password Manager")
        );
        assert_eq!(name("00000000-0000-0000-0000-000000000000"), None);
    }
}
