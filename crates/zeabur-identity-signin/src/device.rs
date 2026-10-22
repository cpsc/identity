//! Who is asking: the browser and address of a request, for default passkey names and the audit
//! log.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};

/// The longest user agent kept in logs and events.
const USER_AGENT_MAX: usize = 256;

/// The requesting browser and address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requester {
    /// The client address: the last `X-Forwarded-For` entry (added by the proxy in front, which
    /// terminates TLS), else the peer address.
    pub ip: Option<IpAddr>,
    /// The TCP peer, normally that proxy.
    pub peer: Option<IpAddr>,
    /// `User-Agent`, shortened.
    pub user_agent: Option<String>,
}

impl Requester {
    /// Reads the requester from headers and the connection's peer address.
    #[must_use]
    pub fn new(headers: &HeaderMap, peer: Option<IpAddr>) -> Self {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .last()
            .and_then(|ip| ip.trim().parse().ok());
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|ua| ua.chars().take(USER_AGENT_MAX).collect());
        Self {
            ip: forwarded.or(peer),
            peer,
            user_agent,
        }
    }

    /// A name for a passkey made in this browser, such as "Chrome (macOS)".
    #[must_use]
    pub fn device_name(&self) -> Option<String> {
        device_name(self.user_agent.as_deref()?)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Requester {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = Result<Self, Infallible>> + Send {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        std::future::ready(Ok(Self::new(&parts.headers, peer)))
    }
}

/// Browser and operating system from a `User-Agent`, for a default label. Order matters: Edge
/// and Opera also say Chrome, Chrome also says Safari, iOS also says Mac OS X.
#[must_use]
pub fn device_name(user_agent: &str) -> Option<String> {
    let has = |needle: &str| user_agent.contains(needle);
    let browser = if has("Edg/") || has("EdgA/") || has("EdgiOS/") {
        "Edge"
    } else if has("OPR/") {
        "Opera"
    } else if has("Firefox/") || has("FxiOS/") {
        "Firefox"
    } else if has("SamsungBrowser/") {
        "Samsung Internet"
    } else if has("Chrome/") || has("CriOS/") {
        "Chrome"
    } else if has("Safari/") {
        "Safari"
    } else {
        ""
    };
    let system = if has("iPhone") {
        "iPhone"
    } else if has("iPad") {
        "iPad"
    } else if has("Android") {
        "Android"
    } else if has("CrOS") {
        "ChromeOS"
    } else if has("Mac OS X") || has("Macintosh") {
        "macOS"
    } else if has("Windows") {
        "Windows"
    } else if has("Linux") {
        "Linux"
    } else {
        ""
    };
    match (browser.is_empty(), system.is_empty()) {
        (false, false) => Some(format!("{browser} ({system})")),
        (false, true) => Some(browser.to_owned()),
        (true, false) => Some(system.to_owned()),
        (true, true) => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn names_common_browsers() {
        let cases = [
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
                Some("Chrome (macOS)"),
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1",
                Some("Safari (iPhone)"),
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0",
                Some("Edge (Windows)"),
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64; rv:144.0) Gecko/20100101 Firefox/144.0",
                Some("Firefox (Linux)"),
            ),
            (
                "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
                Some("Chrome (Android)"),
            ),
            ("curl/8.9.1", None),
        ];
        for (ua, expected) in cases {
            assert_eq!(device_name(ua).as_deref(), expected, "{ua}");
        }
    }

    #[test]
    fn client_address_is_the_proxys_last_entry() {
        let mut headers = HeaderMap::new();
        headers.append(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 198.51.100.7"),
        );
        let peer = Some(IpAddr::from([10, 0, 0, 2]));
        let requester = Requester::new(&headers, peer);
        assert_eq!(requester.ip, Some(IpAddr::from([198, 51, 100, 7])));
        assert_eq!(requester.peer, peer);
        assert_eq!(Requester::new(&HeaderMap::new(), peer).ip, peer);
    }
}
