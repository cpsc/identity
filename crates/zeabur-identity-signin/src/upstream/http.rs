//! The outgoing HTTPS client used to talk to upstream identity providers.

use std::future::Future;
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{Request, Response};
use http_body_util::{BodyExt as _, Full, Limited};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use thiserror::Error;

/// Largest response body accepted from a provider.
const MAX_BODY: usize = 1024 * 1024;
/// Time allowed for one request, including reading the body.
const TIMEOUT: Duration = Duration::from_secs(10);

/// A provider request failed.
#[derive(Debug, Error)]
#[error("upstream request failed: {0}")]
pub struct HttpError(pub String);

/// Sends one request and returns the whole response. A trait so that tests can answer in
/// process.
pub trait HttpClient: Send + Sync + 'static {
    /// Sends `request`.
    fn send(
        &self,
        request: Request<Bytes>,
    ) -> impl Future<Output = Result<Response<Bytes>, HttpError>> + Send;
}

/// HTTPS only, HTTP/1.1, rustls with the given trust roots.
#[derive(Clone)]
pub struct HyperClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
}

impl std::fmt::Debug for HyperClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HyperClient")
    }
}

impl HyperClient {
    /// A client trusting `roots`.
    ///
    /// # Errors
    ///
    /// Returns an error if the TLS configuration cannot be built.
    pub fn new(roots: rustls::RootCertStore) -> Result<Self, HttpError> {
        let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|error| HttpError(error.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_only()
            .enable_http1()
            .build();
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
        })
    }
}

impl HttpClient for HyperClient {
    fn send(
        &self,
        request: Request<Bytes>,
    ) -> impl Future<Output = Result<Response<Bytes>, HttpError>> + Send {
        let client = self.client.clone();
        async move {
            let exchange = async {
                let response = client
                    .request(request.map(Full::new))
                    .await
                    .map_err(|error| HttpError(error.to_string()))?;
                let (parts, body) = response.into_parts();
                let bytes = Limited::new(body, MAX_BODY)
                    .collect()
                    .await
                    .map_err(|error| HttpError(error.to_string()))?
                    .to_bytes();
                Ok(Response::from_parts(parts, bytes))
            };
            tokio::time::timeout(TIMEOUT, exchange)
                .await
                .map_err(|_elapsed| HttpError("timed out".to_owned()))?
        }
    }
}
