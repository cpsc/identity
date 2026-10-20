//! Passkey (WebAuthn Level 3) ceremonies, wrapping webauthn-rs.
//!
//! Policy: discoverable credentials (resident key required, so the browser can offer them
//! through conditional mediation without asking for a username first), user verification
//! required at registration and at every assertion, attestation `none`, relying party ID = the
//! issuer's host, origin = the issuer. The sign counter is not persisted: synced passkeys
//! report 0, a counter is only a signal (WebAuthn Level 3 §6.1.1), and storing it would cost a
//! write per sign-in.

use thiserror::Error;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, DiscoverableKey, Passkey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse, Url, Uuid, Webauthn, WebauthnBuilder,
};
use zeabur_identity_core::config::BaseUrl;

/// A failed ceremony. Details are logged by the caller; users see a generic message.
#[derive(Debug, Error)]
pub enum PasskeyError {
    /// The relying party configuration is invalid (for example an issuer without a host).
    #[error("invalid WebAuthn configuration: {0}")]
    Config(String),
    /// The browser's response did not verify.
    #[error("passkey verification failed: {0}")]
    Verification(String),
    /// The authenticator did not verify the user (WebAuthn UV flag).
    #[error("the authenticator did not verify the user")]
    NotUserVerified,
}

/// The relying party.
pub struct Passkeys {
    webauthn: Webauthn,
}

impl std::fmt::Debug for Passkeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Passkeys")
    }
}

impl Passkeys {
    /// The relying party for `issuer`: RP ID is its host, the only allowed origin is the issuer.
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Config`] if the issuer cannot serve as a WebAuthn origin.
    pub fn new(issuer: &BaseUrl) -> Result<Self, PasskeyError> {
        let origin =
            Url::parse(issuer.as_str()).map_err(|e| PasskeyError::Config(e.to_string()))?;
        let rp_id = origin
            .host_str()
            .ok_or_else(|| PasskeyError::Config("the issuer has no host".to_owned()))?
            .to_owned();
        let webauthn = WebauthnBuilder::new(&rp_id, &origin)
            .and_then(|builder| builder.rp_name("Zeabur Identity").build())
            .map_err(|e| PasskeyError::Config(e.to_string()))?;
        Ok(Self { webauthn })
    }

    /// Starts registering a passkey for the user with WebAuthn user handle `handle`. Returns the
    /// `CredentialCreationOptions` JSON for the browser and the state to keep.
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Verification`] if the library cannot build the challenge.
    pub fn begin_registration(
        &self,
        handle: Uuid,
        username: &str,
        display_name: &str,
    ) -> Result<(serde_json::Value, PasskeyRegistration), PasskeyError> {
        let (options, state) = self
            .webauthn
            .start_passkey_registration(handle, username, display_name, None)
            .map_err(|e| PasskeyError::Verification(e.to_string()))?;
        let mut options =
            serde_json::to_value(options).map_err(|e| PasskeyError::Verification(e.to_string()))?;
        // Sign-in uses discoverable credentials (no username first), so the credential must be
        // resident. webauthn-rs defaults to non-resident for broad compatibility.
        if let Some(selection) = options
            .get_mut("publicKey")
            .and_then(|p| p.get_mut("authenticatorSelection"))
            .and_then(serde_json::Value::as_object_mut)
        {
            selection.insert("requireResidentKey".into(), true.into());
            selection.insert("residentKey".into(), "required".into());
        }
        Ok((options, state))
    }

    /// Verifies the browser's registration response.
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Verification`] if it does not verify against `state`.
    pub fn finish_registration(
        &self,
        response: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
    ) -> Result<Passkey, PasskeyError> {
        self.webauthn
            .finish_passkey_registration(response, state)
            .map_err(|e| PasskeyError::Verification(e.to_string()))
    }

    /// Starts a sign-in with any discoverable credential (conditional mediation).
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Verification`] if the library cannot build the challenge.
    pub fn begin_sign_in(
        &self,
    ) -> Result<(RequestChallengeResponse, DiscoverableAuthentication), PasskeyError> {
        self.webauthn
            .start_discoverable_authentication()
            .map_err(|e| PasskeyError::Verification(e.to_string()))
    }

    /// Reads the user handle and credential ID from an assertion, before it is verified.
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Verification`] if the response carries no valid user handle.
    pub fn identify(
        &self,
        response: &PublicKeyCredential,
    ) -> Result<(Uuid, Vec<u8>), PasskeyError> {
        self.webauthn
            .identify_discoverable_authentication(response)
            .map(|(handle, id)| (handle, id.to_vec()))
            .map_err(|e| PasskeyError::Verification(e.to_string()))
    }

    /// Verifies an assertion made with `passkey`, requiring user verification.
    ///
    /// # Errors
    ///
    /// [`PasskeyError::Verification`] if the signature or client data do not verify;
    /// [`PasskeyError::NotUserVerified`] if UV was not performed.
    pub fn finish_sign_in(
        &self,
        response: &PublicKeyCredential,
        state: DiscoverableAuthentication,
        passkey: &Passkey,
    ) -> Result<(), PasskeyError> {
        let result = self
            .webauthn
            .finish_discoverable_authentication(response, state, &[DiscoverableKey::from(passkey)])
            .map_err(|e| PasskeyError::Verification(e.to_string()))?;
        // The library requires UV under this policy; check again so a library change cannot
        // silently weaken it (Pocket ID advisory
        // GHSA-pgqv-hh75-xx4x: UV was required in the options but not checked).
        if !result.user_verified() {
            return Err(PasskeyError::NotUserVerified);
        }
        Ok(())
    }
}

pub use zeabur_identity_store::passkey_credential_name as credential_name;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_requires_resident_keys_and_uv() {
        let passkeys =
            Passkeys::new(&BaseUrl::parse("issuer", "https://login.example.com").unwrap()).unwrap();
        let (options, _) = passkeys
            .begin_registration(Uuid::nil(), "alice", "Alice")
            .unwrap();
        let json = options;
        let selection = &json["publicKey"]["authenticatorSelection"];
        assert_eq!(selection["residentKey"], "required");
        assert_eq!(selection["requireResidentKey"], true);
        assert_eq!(selection["userVerification"], "required");
        assert_eq!(json["publicKey"]["rp"]["id"], "login.example.com");
        assert_eq!(json["publicKey"]["attestation"], "none");
    }

    #[test]
    fn sign_in_uses_conditional_mediation() {
        let passkeys =
            Passkeys::new(&BaseUrl::parse("issuer", "https://login.example.com").unwrap()).unwrap();
        let (options, _) = passkeys.begin_sign_in().unwrap();
        let json = serde_json::to_value(&options).unwrap();
        assert_eq!(json["mediation"], "conditional");
        assert_eq!(json["publicKey"]["userVerification"], "required");
        assert!(
            json["publicKey"]["allowCredentials"]
                .as_array()
                .is_none_or(Vec::is_empty)
        );
    }

    #[test]
    fn credential_names_are_dns_safe_and_distinct() {
        let a = credential_name(b"one");
        assert_eq!(a.len(), 66);
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        );
        assert_ne!(a, credential_name(b"two"));
    }
}
