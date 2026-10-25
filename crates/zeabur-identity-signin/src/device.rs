//! Who is asking: the browser and address of a request, for the audit log.

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

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

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
