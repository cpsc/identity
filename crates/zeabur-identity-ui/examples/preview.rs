//! Serves every page state with made-up data, for looking at the pages in a browser:
//! `cargo run -p zeabur-identity-ui --example preview`, then open <http://localhost:8090/>.
//! Add `?lang=zh-TW` or `?lang=zh-CN` to any page.

use std::time::Duration;

use axum::Router;
use axum::extract::{Path, RawQuery};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use zeabur_identity_ui::pages::{
    AppLink, ErrorReason, ProviderButton, error_page, login_page, notice_page, page_lang,
    setup_page,
};

const PROVIDERS: [ProviderButton; 1] = [ProviderButton {
    id: "github",
    label: "GitHub",
    icon: "GithubLogo",
}];
const APPS: [AppLink<'static>; 2] = [
    AppLink {
        name: "Zeabur",
        url: "https://zeabur.example.com/",
    },
    AppLink {
        name: "Grafana",
        url: "https://grafana.example.com/",
    },
];
const REASONS: [(&str, ErrorReason, &str); 14] = [
    (
        "request-expired",
        ErrorReason::RequestExpired,
        "invalid_request_uri: The request_uri is invalid, expired, or already used. Start the sign-in again.",
    ),
    (
        "state-mismatch",
        ErrorReason::StateMismatch,
        "invalid_state (github)",
    ),
    (
        "malformed",
        ErrorReason::MalformedRequest,
        "invalid_request: A parameter appears more than once. Send each parameter once.",
    ),
    (
        "unknown-client",
        ErrorReason::UnknownClient,
        "invalid_request: The client_id is missing or not registered.",
    ),
    (
        "redirect-uri",
        ErrorReason::RedirectUri,
        "invalid_request: This redirect_uri is not registered for the client.",
    ),
    (
        "provider-denied",
        ErrorReason::ProviderDenied,
        "provider_denied (github)",
    ),
    (
        "provider-failed",
        ErrorReason::ProviderFailed,
        "provider_verification_failed (google)",
    ),
    (
        "clock-skew",
        ErrorReason::ClockSkew,
        "provider_verification_failed (google): the provider's ID token is not valid: expired",
    ),
    (
        "not-linked",
        ErrorReason::AccountNotLinked,
        "account_unlinked (github)",
    ),
    (
        "already-linked",
        ErrorReason::AccountAlreadyLinked,
        "account_already_linked (github)",
    ),
    (
        "user-unavailable",
        ErrorReason::UserUnavailable,
        "user_unavailable (google)",
    ),
    (
        "setup-link",
        ErrorReason::SetupLinkInvalid,
        "setup_already_used (github)",
    ),
    (
        "server",
        ErrorReason::ServerError,
        "server_error: Internal error. Try again.",
    ),
    (
        "other",
        ErrorReason::Other,
        "unsupported_response_type: Only response_type=code is supported.",
    ),
];

fn lang(query: Option<&str>, headers: &HeaderMap) -> zeabur_identity_ui::i18n::Lang {
    page_lang(query, headers)
}

async fn index() -> Html<String> {
    let pages = [
        "/login",
        "/login/apps",
        "/login/request",
        "/login/hint",
        "/login/elevated",
        "/setup",
        "/setup#token=preview",
        "/linked",
    ]
    .map(String::from);
    let errors = REASONS.iter().map(|(name, ..)| format!("/error/{name}"));
    let mut links = String::new();
    for path in pages.into_iter().chain(errors) {
        for part in ["<li><a href=\"", &path, "\">", &path, "</a></li>"] {
            links.push_str(part);
        }
    }
    Html(format!(
        "<!doctype html><title>Pages</title><ul>{links}</ul>"
    ))
}

async fn login(Path(state): Path<String>, RawQuery(q): RawQuery, h: HeaderMap) -> Response {
    let lang = lang(q.as_deref(), &h);
    match state.as_str() {
        "apps" => login_page(lang, None, None, None, &PROVIDERS, &APPS),
        "request" => login_page(lang, Some("Zeabur"), None, None, &PROVIDERS, &[]),
        "hint" => login_page(lang, Some("Zeabur"), None, Some("admin"), &PROVIDERS, &[]),
        "elevated" => login_page(
            lang,
            Some("Zeabur"),
            Some(Duration::from_mins(5)),
            None,
            &[],
            &[],
        ),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A challenge the browser can wait on for passkey autofill; nothing ever completes it.
async fn login_options() -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"publicKey":{"challenge":"AAAAAAAAAAAAAAAAAAAAAA","timeout":600000,"rpId":"localhost","userVerification":"required"}}"#,
    )
        .into_response()
}

async fn setup_options() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/problem+json")],
        r#"{"type":"urn:zeabur-identity:problem:setup-invalid","title":"","status":400}"#,
    )
        .into_response()
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route(
            "/login",
            get(|RawQuery(q): RawQuery, h: HeaderMap| async move {
                login_page(lang(q.as_deref(), &h), None, None, None, &PROVIDERS, &[])
            }),
        )
        .route("/login/{state}", get(login))
        .route("/login/options", post(login_options))
        .route(
            "/setup",
            get(|RawQuery(q): RawQuery, h: HeaderMap| async move {
                setup_page(lang(q.as_deref(), &h), &PROVIDERS, &APPS)
            }),
        )
        .route("/setup/options", post(setup_options))
        .route(
            "/linked",
            get(|RawQuery(q): RawQuery, h: HeaderMap| async move {
                let lang = lang(q.as_deref(), &h);
                let m = lang.messages();
                notice_page(lang, m.upstream_linked_title, m.upstream_linked_body, &APPS)
            }),
        )
        .route(
            "/error/{reason}",
            get(
                |Path(name): Path<String>, RawQuery(q): RawQuery, h: HeaderMap| async move {
                    REASONS.iter().find(|(n, ..)| *n == name).map_or_else(
                        || StatusCode::NOT_FOUND.into_response(),
                        |(_, reason, detail)| error_page(lang(q.as_deref(), &h), *reason, detail),
                    )
                },
            ),
        )
        .merge(zeabur_identity_ui::assets::router());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8090").await?;
    axum::serve(listener, app).await
}
