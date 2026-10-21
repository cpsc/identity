//! Rendering the pages: sign-in, enrolment, errors and notices. Routing and the data behind each
//! page belong to the caller.

use std::time::Duration;

use askama::Template;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};

use super::assets::{VERSIONS, Versions};
use super::harden;
use super::i18n::{ErrorText, Lang, Messages};

/// A "Continue with …" button for an upstream identity provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderButton {
    /// Path segment of the provider's routes, for example `github`.
    pub id: &'static str,
    /// Name shown on the button.
    pub label: &'static str,
    /// Icon name (see `templates/icons.html`).
    pub icon: &'static str,
}

/// An application the page offers to open, from its client's home URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppLink<'a> {
    /// The client's display name.
    pub name: &'a str,
    /// Where it opens.
    pub url: &'a str,
}

/// Commands the pages show. They run with the cluster admin's kubeconfig, on the server for a
/// single-server install, with `ZEABUR_IDENTITY_ISSUER` set as in the getting-started guide.
const SETUP_LINK_COMMAND: &str = "zeabur-identityctl setup-link --username <name>";
const RESET_COMMAND: &str = "zeabur-identityctl setup-link --username <name> --reset";
const INVITE_COMMAND: &str = "zeabur-identityctl invite --username <name>";
const LOGS_COMMAND: &str = "kubectl -n zeabur-identity logs deploy/zeabur-identity --tail=100";
const CLOCK_COMMAND: &str = "timedatectl status";

/// Why a sign-in stopped, as the error page explains it: a title, the cause and the next step,
/// with the raw detail behind a disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorReason {
    /// The pending sign-in or pushed request expired, was used, or belongs to another browser.
    RequestExpired,
    /// An upstream provider's answer does not match this browser's pending request (`state`
    /// or the sealed cookie).
    StateMismatch,
    /// The authorization request cannot be parsed.
    MalformedRequest,
    /// The `client_id` is missing or not registered.
    UnknownClient,
    /// The `redirect_uri` is missing or not registered for the client.
    RedirectUri,
    /// The upstream provider came back without a code (cancelled or denied).
    ProviderDenied,
    /// The upstream provider could not be reached or its answer did not verify.
    ProviderFailed,
    /// The upstream ID token is expired or issued in the future: usually this server's clock.
    ClockSkew,
    /// No user is linked to the upstream account.
    AccountNotLinked,
    /// The upstream account is linked already.
    AccountAlreadyLinked,
    /// The linked user is disabled or gone.
    UserUnavailable,
    /// The enrolment link is invalid, used or expired.
    SetupLinkInvalid,
    /// Storage or another server-side step failed.
    ServerError,
    /// Anything else.
    Other,
}

impl ErrorReason {
    const fn text(self, m: &Messages) -> ErrorText {
        match self {
            Self::RequestExpired => m.err_request_expired,
            Self::StateMismatch => m.err_state_mismatch,
            Self::MalformedRequest => m.err_malformed,
            Self::UnknownClient => m.err_unknown_client,
            Self::RedirectUri => m.err_redirect_uri,
            Self::ProviderDenied => m.err_provider_denied,
            Self::ProviderFailed => m.err_provider_failed,
            Self::ClockSkew => m.err_clock_skew,
            Self::AccountNotLinked => m.err_account_not_linked,
            Self::AccountAlreadyLinked => m.err_account_already_linked,
            Self::UserUnavailable => m.err_user_unavailable,
            Self::SetupLinkInvalid => m.err_setup_link,
            Self::ServerError => m.err_server,
            Self::Other => ErrorText {
                title: m.error_title,
                cause: "",
                next: m.error_body,
            },
        }
    }

    const fn command(self) -> Option<&'static str> {
        match self {
            Self::ProviderFailed | Self::ServerError => Some(LOGS_COMMAND),
            Self::ClockSkew => Some(CLOCK_COMMAND),
            Self::RequestExpired
            | Self::StateMismatch
            | Self::MalformedRequest
            | Self::UnknownClient
            | Self::RedirectUri
            | Self::ProviderDenied
            | Self::AccountNotLinked
            | Self::AccountAlreadyLinked
            | Self::UserUnavailable
            | Self::SetupLinkInvalid
            | Self::Other => None,
        }
    }
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage<'a> {
    lang: Lang,
    langs: &'static [Lang],
    m: &'static Messages,
    assets: &'a Versions,
    client: Option<&'a str>,
    subtitle: String,
    elevation_notice: Option<String>,
    login_hint: Option<&'a str>,
    providers: &'a [ProviderButton],
    apps: &'a [AppLink<'a>],
    reset_command: &'static str,
}

#[derive(Template)]
#[template(path = "setup.html")]
struct SetupPage<'a> {
    lang: Lang,
    langs: &'static [Lang],
    m: &'static Messages,
    assets: &'a Versions,
    providers: &'a [ProviderButton],
    apps: &'a [AppLink<'a>],
    setup_link_command: &'static str,
    invite_command: &'static str,
}

#[derive(Template)]
#[template(path = "notice.html")]
struct NoticePage<'a> {
    lang: Lang,
    langs: &'static [Lang],
    m: &'static Messages,
    assets: &'a Versions,
    title: &'a str,
    body: &'a str,
    apps: &'a [AppLink<'a>],
}

/// Renders a page that only informs (for example, "account linked"), with a way into `apps`.
#[must_use]
pub fn notice_page(lang: Lang, title: &str, body: &str, apps: &[AppLink<'_>]) -> Response {
    let page = NoticePage {
        lang,
        langs: &Lang::ALL,
        m: lang.messages(),
        assets: &VERSIONS,
        title,
        body,
        apps,
    };
    render(StatusCode::OK, &page)
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage<'a> {
    lang: Lang,
    langs: &'static [Lang],
    m: &'static Messages,
    assets: &'a Versions,
    text: ErrorText,
    command: Option<&'static str>,
    new_link: bool,
    setup_link_command: &'static str,
    detail: &'a str,
}

/// The page language: `?lang=` (the language switcher), then `Accept-Language`.
#[must_use]
pub fn page_lang(query: Option<&str>, headers: &HeaderMap) -> Lang {
    let explicit = form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .find(|(key, _)| key == "lang")
        .map(|(_, value)| value.into_owned());
    Lang::negotiate(explicit.as_deref(), accept_language(headers))
}

/// The `Accept-Language` header, if it is text.
#[must_use]
pub fn accept_language(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
}

fn render(status: StatusCode, page: &impl Template) -> Response {
    let mut response = match page.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(error) => {
            tracing::error!(%error, reason = "page_render_failed", "rendering a page failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    };
    harden(&mut response);
    response
}

/// Renders the error page for an authorization request that cannot be trusted enough to redirect
/// (RFC 6749 §4.1.2.1) or an upstream sign-in that failed.
///
/// It says what happened and what to do for `reason`, then `detail` (for the administrator)
/// behind a collapsed disclosure.
#[must_use]
pub fn error_page(lang: Lang, reason: ErrorReason, detail: &str) -> Response {
    let m = lang.messages();
    let page = ErrorPage {
        lang,
        langs: &Lang::ALL,
        m,
        assets: &VERSIONS,
        text: reason.text(m),
        command: reason.command(),
        new_link: reason == ErrorReason::SetupLinkInvalid,
        setup_link_command: SETUP_LINK_COMMAND,
        detail,
    };
    render(StatusCode::BAD_REQUEST, &page)
}

/// Renders the sign-in page. Without a pending request (`client` is `None`) it explains that
/// there is nothing to sign in to and offers `apps`; `providers` adds "Continue with …" buttons.
///
/// `elevation` is the elevated token's lifetime when the request asks for one; the page then
/// says so before the passkey prompt.
#[must_use]
pub fn login_page(
    lang: Lang,
    client: Option<&str>,
    elevation: Option<Duration>,
    login_hint: Option<&str>,
    providers: &[ProviderButton],
    apps: &[AppLink<'_>],
) -> Response {
    let m = lang.messages();
    let subtitle = client.map_or_else(
        || m.login_subtitle.to_owned(),
        |c| format!("{} {c}", m.login_continue_to),
    );
    let elevation_notice = client.zip(elevation).map(|(client, lifetime)| {
        // Whole minutes, rounded up, so the page never understates how long access lasts.
        let minutes = lifetime.as_secs().div_ceil(60).max(1);
        // `%minutes%` first, so a display name containing it is shown verbatim.
        m.elevated_body
            .replace("%minutes%", &minutes.to_string())
            .replace("%app%", client)
    });
    let page = LoginPage {
        lang,
        langs: &Lang::ALL,
        m,
        assets: &VERSIONS,
        client,
        subtitle,
        elevation_notice,
        login_hint,
        providers,
        apps,
        reset_command: RESET_COMMAND,
    };
    render(StatusCode::OK, &page)
}

/// Renders the enrolment page, where a setup link or invitation becomes a passkey (or, with
/// `providers`, a linked upstream account). Once done, it offers `apps` and a backup passkey.
#[must_use]
pub fn setup_page(lang: Lang, providers: &[ProviderButton], apps: &[AppLink<'_>]) -> Response {
    let page = SetupPage {
        lang,
        langs: &Lang::ALL,
        m: lang.messages(),
        assets: &VERSIONS,
        providers,
        apps,
        setup_link_command: SETUP_LINK_COMMAND,
        invite_command: INVITE_COMMAND,
    };
    render(StatusCode::OK, &page)
}
