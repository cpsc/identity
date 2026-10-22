#![doc = include_str!("../README.md")]

pub mod aaguid;
pub mod assets;
pub mod i18n;
pub mod pages;

use axum::http::{HeaderValue, header};
use axum::response::Response;

/// Content Security Policy for every HTML page: nothing inline, nothing from other origins, no
/// framing, and Trusted Types with no policies (the scripts never write HTML).
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; \
connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; \
require-trusted-types-for 'script'; trusted-types 'none'";

/// Adds the security headers every HTML page carries.
pub fn harden(response: &mut Response) {
    let headers = response.headers_mut();
    let mut set = |name, value: &'static str| {
        headers.insert(name, HeaderValue::from_static(value));
    };
    set(header::CONTENT_SECURITY_POLICY, CSP);
    set(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(header::X_FRAME_OPTIONS, "DENY");
    set(header::REFERRER_POLICY, "no-referrer");
    set(header::CACHE_CONTROL, "no-store");
    set(
        header::HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    set(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        "same-origin",
    );
    set(
        header::HeaderName::from_static("permissions-policy"),
        "publickey-credentials-get=(self), publickey-credentials-create=(self), camera=(), microphone=(), geolocation=()",
    );
}
