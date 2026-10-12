//! Static assets, built by `scripts/build-ui.sh`, committed, and embedded in the binary. Nothing is
//! loaded from other origins at run time.

use std::sync::LazyLock;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use genkan_core::crypto::{hex, sha256};

struct Asset {
    name: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

const ASSETS: [Asset; 4] = [
    Asset {
        name: "app.css",
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/app.css"),
    },
    Asset {
        name: "login.js",
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/login.js"),
    },
    Asset {
        name: "setup.js",
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/setup.js"),
    },
    Asset {
        name: "geist-latin-wght-normal.woff2",
        content_type: "font/woff2",
        bytes: include_bytes!("../assets/geist-latin-wght-normal.woff2"),
    },
];

/// Content hashes appended to asset URLs, so they can be cached forever.
#[derive(Debug, Clone)]
pub struct Versions {
    /// `app.css`.
    pub css: String,
    /// `login.js`.
    pub login_js: String,
    /// `setup.js`.
    pub setup_js: String,
}

/// The embedded assets' versions, hashed once.
pub static VERSIONS: LazyLock<Versions> = LazyLock::new(Versions::compute);

impl Versions {
    fn compute() -> Self {
        let version = |name: &str| {
            ASSETS
                .iter()
                .find(|a| a.name == name)
                .map_or_else(String::new, |a| {
                    hex(&sha256(a.bytes)).chars().take(16).collect()
                })
        };
        Self {
            css: version("app.css"),
            login_js: version("login.js"),
            setup_js: version("setup.js"),
        }
    }
}

/// `GET /assets/{name}`, for every embedded asset.
pub fn router() -> Router {
    Router::new().route("/assets/{name}", get(serve))
}

/// `GET /assets/{name}`.
pub async fn serve(Path(name): Path<String>) -> Response {
    let Some(asset) = ASSETS.iter().find(|a| a.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = asset.bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    response
}
