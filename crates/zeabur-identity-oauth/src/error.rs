//! OAuth error codes and responses (RFC 6749 §4.1.2.1 and §5.2, OIDC Core §3.1.2.6, RFC 8707,
//! RFC 7009, RFC 9126).

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Registered error codes used by this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// RFC 6749.
    InvalidRequest,
    /// RFC 6749.
    InvalidClient,
    /// RFC 6749.
    InvalidGrant,
    /// RFC 6749.
    UnauthorizedClient,
    /// RFC 6749.
    UnsupportedGrantType,
    /// RFC 6749.
    UnsupportedResponseType,
    /// RFC 6749.
    InvalidScope,
    /// RFC 6749.
    AccessDenied,
    /// RFC 6749.
    ServerError,
    /// RFC 6749.
    TemporarilyUnavailable,
    /// RFC 8707.
    InvalidTarget,
    /// OIDC Core §3.1.2.6.
    LoginRequired,
    /// OIDC Core §3.1.2.6.
    RequestNotSupported,
    /// OIDC Core §3.1.2.6.
    RequestUriNotSupported,
    /// OIDC Core §3.1.2.6.
    InvalidRequestUri,
    /// OIDC Core §3.1.2.6.
    RegistrationNotSupported,
    /// RFC 7009.
    UnsupportedTokenType,
}

impl ErrorCode {
    /// The wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::UnsupportedGrantType => "unsupported_grant_type",
            Self::UnsupportedResponseType => "unsupported_response_type",
            Self::InvalidScope => "invalid_scope",
            Self::AccessDenied => "access_denied",
            Self::ServerError => "server_error",
            Self::TemporarilyUnavailable => "temporarily_unavailable",
            Self::InvalidTarget => "invalid_target",
            Self::LoginRequired => "login_required",
            Self::RequestNotSupported => "request_not_supported",
            Self::RequestUriNotSupported => "request_uri_not_supported",
            Self::InvalidRequestUri => "invalid_request_uri",
            Self::RegistrationNotSupported => "registration_not_supported",
            Self::UnsupportedTokenType => "unsupported_token_type",
        }
    }
}

/// An error with a fixed, human-readable description. Descriptions never echo request input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OAuthError {
    /// Error code.
    pub error: ErrorCode,
    /// Explanation for developers (RFC 6749 §5.2 `error_description`).
    pub error_description: &'static str,
}

impl OAuthError {
    /// Creates an error.
    #[must_use]
    pub const fn new(error: ErrorCode, description: &'static str) -> Self {
        Self {
            error,
            error_description: description,
        }
    }

    /// `server_error`, for storage or crypto failures. Details are logged, not returned.
    #[must_use]
    pub const fn server() -> Self {
        Self::new(ErrorCode::ServerError, "internal error")
    }

    /// The HTTP status for a token-endpoint style JSON error (RFC 6749 §5.2).
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self.error {
            ErrorCode::InvalidClient => StatusCode::UNAUTHORIZED,
            ErrorCode::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::TemporarilyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::InvalidRequest
            | ErrorCode::InvalidGrant
            | ErrorCode::UnauthorizedClient
            | ErrorCode::UnsupportedGrantType
            | ErrorCode::UnsupportedResponseType
            | ErrorCode::InvalidScope
            | ErrorCode::AccessDenied
            | ErrorCode::InvalidTarget
            | ErrorCode::LoginRequired
            | ErrorCode::RequestNotSupported
            | ErrorCode::RequestUriNotSupported
            | ErrorCode::InvalidRequestUri
            | ErrorCode::RegistrationNotSupported
            | ErrorCode::UnsupportedTokenType => StatusCode::BAD_REQUEST,
        }
    }
}

/// Headers every response carrying tokens or errors from the token, PAR and revocation endpoints
/// must have (RFC 6749 §5.1: `Cache-Control: no-store`).
pub fn no_store(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        let mut response = (self.status(), axum::Json(self)).into_response();
        no_store(&mut response);
        response
    }
}
