//! RFC 9457 problem details for Genkan's own JSON endpoints (not the OAuth endpoints, which use
//! RFC 6749 errors).

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
            "Something went wrong. Try again.",
        )
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let body = Body {
            // RFC 9457 §3.1.1: a URI reference identifying the problem type.
            kind: format!("urn:genkan:problem:{}", self.kind),
            title: self.title,
            status: self.status.as_u16(),
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
