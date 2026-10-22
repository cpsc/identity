//! RFC 9457 problem details for Zeabur Identity's own JSON endpoints (not the OAuth endpoints,
//! which use RFC 6749 errors).

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A problem. `kind` is a short stable code the UI can switch on; `detail` is for people.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Problem {
    /// HTTP status.
    pub status: StatusCode,
    /// Stable machine-readable code, also used to build `type`.
    pub kind: &'static str,
    /// Short summary.
    pub title: &'static str,
}

#[derive(Serialize)]
struct Body {
    #[serde(rename = "type")]
    kind: String,
    title: &'static str,
    status: u16,
    #[serde(flatten)]
    extensions: serde_json::Map<String, serde_json::Value>,
}

impl Problem {
    /// Creates a problem.
    #[must_use]
    pub const fn new(status: StatusCode, kind: &'static str, title: &'static str) -> Self {
        Self {
            status,
            kind,
            title,
        }
    }

    /// An internal error; details are logged, not returned.
    #[must_use]
    pub const fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Unable to complete the request. Try again.",
        )
    }

    /// The response with an extension member (RFC 9457 §3.2).
    #[must_use]
    pub fn with(self, name: &str, value: serde_json::Value) -> Response {
        let mut extensions = serde_json::Map::new();
        extensions.insert(name.to_owned(), value);
        self.respond(extensions)
    }

    fn respond(self, extensions: serde_json::Map<String, serde_json::Value>) -> Response {
        let body = Body {
            // RFC 9457 §3.1.1: a URI reference identifying the problem type.
            kind: format!("urn:zeabur-identity:problem:{}", self.kind),
            title: self.title,
            status: self.status.as_u16(),
            extensions,
        };
        let mut response = (self.status, axum::Json(body)).into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        self.respond(serde_json::Map::new())
    }
}
