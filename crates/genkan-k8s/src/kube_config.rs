//! `genkanctl kube-config`: the kube-apiserver side of the integration.
//!
//! Run by the cluster admin with their own kubeconfig. It finds Genkan's Service, makes sure the
//! internal listener has a certificate for the Service's ClusterIP (issuing one if needed,
//! [`crate::internal_tls`]), and prints the `AuthenticationConfiguration` for the chosen
//! distribution together with how to install it there.
//!
//! The file is a structured `AuthenticationConfiguration` (`apiserver.config.k8s.io/v1`, GA in
//! Kubernetes 1.34, [KEP-3331]). kube-apiserver re-reads it about once a minute, so only the
//! flag that names it needs a restart. It cannot be combined with `--oidc-*` flags.
//!
//! # Discovery from inside the cluster
//!
//! With [`DiscoveryMode::Internal`] (the default), the control plane does not depend on public
//! DNS, the ingress or a publicly trusted certificate. kube-apiserver redirects only the
//! discovery request to `discoveryURL`; the document it gets there must still name the public
//! issuer, and the JWK set is fetched from the document's `jwks_uri` ([oidc.go]). Genkan's
//! internal listener therefore serves a copy of the metadata that differs only in `jwks_uri`.
//! kube-apiserver resolves names with the host's resolver, so cluster DNS names do not resolve
//! ([DNS]); it uses the Service's ClusterIP, over `https` (required by [validation.go]), with
//! Genkan's own CA as `certificateAuthority` ([`crate::internal_tls`]). kube-apiserver starts even
//! while Genkan is down and retries discovery every 10 s; it fetches discovery once, when the
//! authenticator starts. A static JWK set is not an option: KEP-3331 lists other key discovery
//! methods as a non-goal. `egressSelectorType` is never set: k3s dials a host name through it as
//! a node name, and kubeadm has no egress configuration, so the authenticator would fail.
//!
//! # Rules kube-apiserver enforces
//!
//! - `has(claims.client_id)`: kube-apiserver does not look at the JWT `typ`, so this accepts access
//!   tokens, which carry `client_id` ([RFC 9068] §2.2), and rejects ID tokens, which are meant for
//!   the client.
//! - `exp - iat` is bounded, because kube-apiserver cannot revoke a token before it expires.
//! - A token whose groups include `elevated:` must be short and issued within two minutes of the
//!   sign-in (`auth_time`), so a mistake in the issuer cannot hand out long-lived privilege. JSON
//!   claims are untyped in CEL, so the list is wrapped in `dyn(claims.groups)` for the
//!   comprehension to type-check; this was verified on a real API server.
//! - Usernames and groups get the `genkan:` prefix, which already rules out `system:`. Structured
//!   configuration has no built-in rejection of `system:`, so `userValidationRules` reject it as
//!   well, in case the mapping changes.
//! - `client_id`, `amr` and `jti` are mapped into `extra` under `genkan.engawahq.dev/`, so `kubectl
//!   auth whoami` and audit events tie a request to one token issuance.
//!
//! # Distributions
//!
//! | `--distro` | `anonymous` section | Internal discovery |
//! |---|---|---|
//! | `k3s` | `enabled: false` | With the embedded agent |
//! | `k0s` | omitted | With a worker on the controller |
//! | `kubeadm`, kind | health paths only | Yes |
//! | `kubesolo` | unsupported | - |
//! | `generic` | omitted | Depends on the cluster |
//!
//! - **k3s** normally passes `--anonymous-auth=false` but drops it when it sees an authentication
//!   configuration file, so the file restores it. Internal discovery needs kube-proxy on the
//!   server, which `--disable-agent` removes.
//! - **k0s** keeps `--anonymous-auth=false`, and releases before v1.34.11+k0s.0, v1.35.8+k0s.0 and
//!   v1.36.3+k0s.1 refuse to start kube-apiserver when the file has an `anonymous` field.
//!   kube-apiserver runs as the user `kube-apiserver`, so the file must be readable by it (mode
//!   0644; it holds no secrets). Dedicated controllers run no kube-proxy and use public discovery.
//! - **kubeadm** runs kube-apiserver as a static pod whose probes the kubelet sends without
//!   credentials, so anonymous access stays on for `/livez`, `/readyz` and `/healthz` only. The
//!   directory, not the file, is mounted into the pod: a single-file bind mount would keep showing
//!   the old file after an atomic replace.
//! - **KubeSolo** v1.2.1 hard-codes kube-apiserver's flags, so the file cannot be passed
//!   ([KubeSolo]).
//!
//! Installation follows each distribution's documented mechanism ([k3s], [k0s], [kubeadm]); see
//! [`instructions`].
//!
//! # References
//!
//! - [KEP-3331] The Kubernetes Authors, "KEP-3331: Structured Authentication Configuration,"
//!   kubernetes/enhancements.
//! - [oidc.go] The Kubernetes Authors,
//!   `staging/src/k8s.io/apiserver/plugin/pkg/authenticator/token/oidc/oidc.go`.
//! - [validation.go] The Kubernetes Authors,
//!   `staging/src/k8s.io/apiserver/pkg/apis/apiserver/validation/validation.go`, v1.37.1.
//! - [DNS] The Kubernetes Authors, "DNS for Services and Pods," Kubernetes documentation.
//! - [RFC 9068] V. Bertocci, "JSON Web Token (JWT) Profile for OAuth 2.0 Access Tokens," RFC 9068,
//!   IETF, Oct. 2021.
//! - [k3s] K3s project, K3s documentation, `k3s server` command reference.
//! - [k0s] k0s project, k0s documentation, configuration reference.
//! - [kubeadm] The Kubernetes Authors, Kubernetes documentation, kubeadm control-plane flags.
//! - [KubeSolo] Portainer, KubeSolo, GitHub repository.
//!
//! [KEP-3331]: https://github.com/kubernetes/enhancements/tree/master/keps/sig-auth/3331-structured-authentication-configuration
//! [oidc.go]: https://github.com/kubernetes/kubernetes/blob/master/staging/src/k8s.io/apiserver/plugin/pkg/authenticator/token/oidc/oidc.go
//! [validation.go]: https://github.com/kubernetes/kubernetes/blob/v1.37.1/staging/src/k8s.io/apiserver/pkg/apis/apiserver/validation/validation.go
//! [DNS]: https://kubernetes.io/docs/concepts/services-networking/dns-pod-service/
//! [RFC 9068]: https://www.rfc-editor.org/rfc/rfc9068
//! [k3s]: https://docs.k3s.io/cli/server
//! [k0s]: https://docs.k0sproject.io/stable/configuration/
//! [kubeadm]: https://kubernetes.io/docs/setup/production-environment/tools/kubeadm/control-plane-flags/
//! [KubeSolo]: https://github.com/portainer/kubesolo

use std::net::IpAddr;
use std::time::Duration;

use clap::ValueEnum;
use genkan_core::clock::UnixTime;
use genkan_core::config::{BaseUrl, ConfigError};
use genkan_core::protocol::{ELEVATED_GROUP_PREFIX, OPENID_CONFIGURATION_PATH};
use k8s_openapi::api::core::v1::{Secret, Service};
use kube::api::{Api, PostParams};
use kube::{Client, ResourceExt as _};
use thiserror::Error;

use crate::internal_tls::{self, TlsError};
use crate::rbac::{IDENTITY_PREFIX, NAME};

/// Prefix of the `extra` keys Genkan maps into Kubernetes user info.
pub const EXTRA_PREFIX: &str = "genkan.engawahq.dev/";

/// Most seconds between sign-in and an elevated token's issue: the code lifetime with slack.
const ELEVATED_FRESHNESS: u64 = 120;

/// Renew a reused certificate that expires sooner than this.
const RENEW_BEFORE: Duration = Duration::from_hours(30 * 24);

/// Where kube-apiserver runs; decides the `anonymous` section and the install instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Distro {
    /// k3s (kube-apiserver inside the k3s process).
    K3s,
    /// k0s with a worker on each controller (`--enable-worker` or `--single`).
    K0s,
    /// kubeadm, and kind, which uses it (kube-apiserver as a static pod).
    Kubeadm,
    /// KubeSolo: not supported, it cannot pass flags to kube-apiserver.
    Kubesolo,
    /// Any other distribution: the configuration file only.
    Generic,
}

/// How kube-apiserver reaches discovery and the JWK set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DiscoveryMode {
    /// Through the Service's ClusterIP, with Genkan's own certificate (no dependency on public
    /// DNS, ingress or a public certificate).
    Internal,
    /// Through the public issuer URL, which the control-plane nodes must reach and trust.
    Public,
}

/// Internal discovery: where, and the CA that vouches for it.
#[derive(Debug, Clone)]
pub struct InternalDiscovery {
    /// Base URL of the internal listener (`https://<ClusterIP>`).
    pub url: BaseUrl,
    /// PEM of the CA kube-apiserver trusts for that URL.
    pub ca_pem: String,
}

/// Everything the generated `AuthenticationConfiguration` depends on.
#[derive(Debug, Clone)]
pub struct ApiserverOptions {
    /// The distribution.
    pub distro: Distro,
    /// The issuer identifier.
    pub issuer: BaseUrl,
    /// Internal discovery, or `None` for the public URL.
    pub internal: Option<InternalDiscovery>,
    /// Accepted token audiences.
    pub audiences: Vec<String>,
    /// Longest accepted token lifetime (`exp - iat`).
    pub max_token_lifetime: Duration,
    /// Longest accepted lifetime of a token carrying `elevated:` groups.
    pub max_elevated_token_lifetime: Duration,
}

/// Why `kube-config` failed.
#[derive(Debug, Error)]
pub enum KubeConfigError {
    /// The distribution cannot be configured.
    #[error(
        "KubeSolo hard-codes kube-apiserver's flags, so it cannot use an AuthenticationConfiguration yet"
    )]
    Unsupported,
    /// The Kubernetes API failed.
    #[error("Kubernetes API: {0}")]
    Kubernetes(#[from] kube::Error),
    /// Genkan's Service is missing or has no ClusterIP.
    #[error(
        "Service {namespace}/{name} has no ClusterIP; install Genkan first (`genkanctl manifests`)"
    )]
    NoService {
        /// Namespace searched.
        namespace: String,
        /// Service name.
        name: &'static str,
    },
    /// The ClusterIP is not an IP address.
    #[error("Service ClusterIP {0:?} is not an IP address")]
    BadClusterIp(String),
    /// Issuing or reading the certificate failed.
    #[error("internal certificate: {0}")]
    Tls(#[from] TlsError),
    /// The existing Secret carries no CA certificate to give kube-apiserver.
    #[error(
        "Secret {0} has no ca.crt; re-run with --renew-certificate, or add the CA that issued it"
    )]
    NoCa(&'static str),
    /// An option is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// An audience cannot be used.
    #[error("audience {0:?} must be non-empty printable ASCII without spaces")]
    BadAudience(String),
}

/// A YAML double-quoted scalar (JSON string syntax is valid YAML).
fn quoted(value: &str) -> String {
    serde_json::Value::String(value.to_owned()).to_string()
}

/// Renders the `AuthenticationConfiguration` (YAML).
///
/// # Errors
///
/// Returns [`KubeConfigError::Unsupported`] for KubeSolo, and [`KubeConfigError::BadAudience`]
/// for an empty or unusual audience.
pub fn authentication_configuration(options: &ApiserverOptions) -> Result<String, KubeConfigError> {
    if options.distro == Distro::Kubesolo {
        return Err(KubeConfigError::Unsupported);
    }
    if let Some(bad) = options
        .audiences
        .iter()
        .find(|a| a.is_empty() || !a.bytes().all(|b| b.is_ascii_graphic()))
    {
        return Err(KubeConfigError::BadAudience(bad.clone()));
    }
    let mut out = String::new();
    let mut line = |text: &str| {
        out.push_str(text);
        out.push('\n');
    };
    line("# kube-apiserver AuthenticationConfiguration for Genkan, from `genkanctl kube-config`.");
    line("apiVersion: apiserver.config.k8s.io/v1");
    line("kind: AuthenticationConfiguration");
    match options.distro {
        // k3s drops its own --anonymous-auth=false when it sees this file; restore it.
        Distro::K3s => {
            line("anonymous:");
            line("  enabled: false");
        }
        // kubelet probes the static pod anonymously.
        Distro::Kubeadm => {
            line("anonymous:");
            line("  enabled: true");
            line("  conditions:");
            for path in ["/livez", "/readyz", "/healthz"] {
                line(&format!("  - path: {path}"));
            }
        }
        // k0s keeps --anonymous-auth=false; older k0s refuses the field.
        Distro::K0s | Distro::Generic | Distro::Kubesolo => {}
    }
    line("jwt:");
    line("- issuer:");
    line(&format!("    url: {}", quoted(options.issuer.as_str())));
    if let Some(internal) = &options.internal {
        line(&format!(
            "    discoveryURL: {}",
            quoted(&internal.url.join(OPENID_CONFIGURATION_PATH))
        ));
        line("    certificateAuthority: |");
        for pem in internal.ca_pem.lines().filter(|l| !l.trim().is_empty()) {
            line(&format!("      {}", pem.trim_end()));
        }
    }
    line("    audiences:");
    for audience in &options.audiences {
        line(&format!("    - {}", quoted(audience)));
    }
    line("    audienceMatchPolicy: MatchAny");
    line("  claimValidationRules:");
    line("  - expression: \"has(claims.client_id)\"");
    line("    message: \"only Genkan access tokens are accepted (ID tokens carry no client_id)\"");
    let lifetime = options.max_token_lifetime.as_secs();
    line(&format!(
        "  - expression: \"claims.exp - claims.iat <= {lifetime}\""
    ));
    line(&format!(
        "    message: \"token lifetime must not exceed {lifetime} seconds\""
    ));
    // Elevated groups only in short tokens issued right after a sign-in, enforced here too, so a
    // mistake in the issuer cannot hand out long-lived privilege. JSON claims are untyped in CEL;
    // `dyn()` lets the comprehension accept the list.
    let elevated = options.max_elevated_token_lifetime.as_secs();
    line(&format!(
        "  - expression: \"!has(claims.groups) || !dyn(claims.groups).exists(g, g.startsWith('{ELEVATED_GROUP_PREFIX}')) || (claims.exp - claims.iat <= {elevated} && claims.iat - claims.auth_time <= {ELEVATED_FRESHNESS})\""
    ));
    line(&format!(
        "    message: \"elevated groups need a token of at most {elevated} seconds issued right after a passkey sign-in\""
    ));
    line("  claimMappings:");
    line("    username:");
    line("      claim: preferred_username");
    line(&format!("      prefix: {}", quoted(IDENTITY_PREFIX)));
    line("    groups:");
    line("      claim: groups");
    line(&format!("      prefix: {}", quoted(IDENTITY_PREFIX)));
    line("    uid:");
    line("      claim: sub");
    line("    extra:");
    line(&format!(
        "    - key: {}",
        quoted(&format!("{EXTRA_PREFIX}client-id"))
    ));
    line("      valueExpression: \"claims.client_id\"");
    line(&format!(
        "    - key: {}",
        quoted(&format!("{EXTRA_PREFIX}amr"))
    ));
    line("      valueExpression: \"has(claims.amr) ? claims.amr : []\"");
    line(&format!(
        "    - key: {}",
        quoted(&format!("{EXTRA_PREFIX}jti"))
    ));
    line("      valueExpression: \"claims.jti\"");
    line("  userValidationRules:");
    line("  - expression: \"!user.username.startsWith('system:')\"");
    line("    message: \"the system: prefix is reserved\"");
    line("  - expression: \"user.groups.all(g, !g.startsWith('system:'))\"");
    line("    message: \"the system: prefix is reserved\"");
    Ok(out)
}

/// How to install the file on `distro`, for the cluster admin (plain text).
#[must_use]
pub fn instructions(distro: Distro, discovery: DiscoveryMode) -> String {
    let (path, steps): (&str, &[&str]) = match distro {
        Distro::K3s => (
            "/etc/rancher/k3s/genkan/authn.yaml",
            &[
                "On every server node, save the configuration above as /etc/rancher/k3s/genkan/authn.yaml.",
                "Add /etc/rancher/k3s/config.yaml.d/50-genkan.yaml containing:",
                "    kube-apiserver-arg:",
                "      - \"authentication-config=/etc/rancher/k3s/genkan/authn.yaml\"",
                "Run `systemctl restart k3s` once. Later changes to authn.yaml are picked up within about a minute.",
                "Internal discovery needs the node's embedded agent (it does not work with --disable-agent).",
            ],
        ),
        Distro::K0s => (
            "/etc/k0s/genkan/authn.yaml",
            &[
                "On every controller, save the configuration above as /etc/k0s/genkan/authn.yaml with mode 0644 (kube-apiserver runs as user kube-apiserver).",
                "In /etc/k0s/k0s.yaml set:",
                "    spec:",
                "      api:",
                "        extraArgs:",
                "          authentication-config: /etc/k0s/genkan/authn.yaml",
                "Remove any oidc-* extraArgs (they cannot be combined with authentication-config).",
                "Run `systemctl restart k0scontroller` on each controller, one at a time.",
                "Internal discovery needs a worker on the controller (--enable-worker or --single); dedicated controllers should use --discovery public.",
            ],
        ),
        Distro::Kubeadm => (
            "/etc/kubernetes/genkan/authn.yaml",
            &[
                "On every control-plane node, save the configuration above as /etc/kubernetes/genkan/authn.yaml.",
                "Add to the ClusterConfiguration (kubeadm.k8s.io/v1beta4, `kubectl -n kube-system edit cm kubeadm-config`):",
                "    apiServer:",
                "      extraArgs:",
                "      - name: authentication-config",
                "        value: /etc/kubernetes/genkan/authn.yaml",
                "      extraVolumes:",
                "      - name: genkan",
                "        hostPath: /etc/kubernetes/genkan",
                "        mountPath: /etc/kubernetes/genkan",
                "        readOnly: true",
                "        pathType: DirectoryOrCreate",
                "Then on each control-plane node: `kubeadm init phase control-plane apiserver --config <file>`.",
                "The directory (not the file) is mounted so that replacing authn.yaml is seen by kube-apiserver.",
            ],
        ),
        Distro::Generic | Distro::Kubesolo => (
            "authn.yaml",
            &[
                "Save the configuration above where kube-apiserver can read it and start kube-apiserver with --authentication-config=<path>.",
                "Remove any --oidc-* flags; they cannot be combined with --authentication-config.",
                "Check how your distribution handles --anonymous-auth: the file may need an `anonymous` section.",
            ],
        ),
    };
    let mut lines = vec![format!("Install ({path}):")];
    lines.extend(steps.iter().map(|step| format!("  {step}")));
    if discovery == DiscoveryMode::Public {
        lines.push(
            "  Public discovery: every control-plane node must resolve the issuer and trust its certificate."
                .to_owned(),
        );
    }
    lines.push(format!(
        "Check: `kubectl --token=<access token> auth whoami` shows a user named {IDENTITY_PREFIX}<username>."
    ));
    lines.join("\n")
}

/// Inputs of [`prepare`].
#[derive(Debug, Clone)]
pub struct Request {
    /// Genkan's namespace.
    pub namespace: String,
    /// What to generate.
    pub distro: Distro,
    /// The issuer identifier.
    pub issuer: BaseUrl,
    /// How kube-apiserver reaches discovery.
    pub discovery: DiscoveryMode,
    /// Accepted audiences.
    pub audiences: Vec<String>,
    /// Longest accepted token lifetime.
    pub max_token_lifetime: Duration,
    /// Longest accepted lifetime of an elevated token.
    pub max_elevated_token_lifetime: Duration,
    /// Issue a new certificate even if a usable one exists.
    pub renew: bool,
    /// Lifetime of a newly issued certificate.
    pub certificate_validity: Duration,
    /// The current time.
    pub now: UnixTime,
}

/// What [`prepare`] produced.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The `AuthenticationConfiguration` (YAML).
    pub configuration: String,
    /// When a newly issued certificate expires, if one was issued.
    pub issued_until: Option<UnixTime>,
}

/// Prepares the cluster for internal discovery (if chosen) and renders the configuration.
///
/// # Errors
///
/// Returns [`KubeConfigError`] if the distribution is unsupported, Genkan's Service is missing,
/// or the API refuses a request.
pub async fn prepare(client: Client, request: &Request) -> Result<Prepared, KubeConfigError> {
    if request.distro == Distro::Kubesolo {
        return Err(KubeConfigError::Unsupported);
    }
    let (internal, issued_until) = match request.discovery {
        DiscoveryMode::Public => (None, None),
        DiscoveryMode::Internal => {
            let (internal, issued) = ensure_certificate(client, request).await?;
            (Some(internal), issued)
        }
    };
    let configuration = authentication_configuration(&ApiserverOptions {
        distro: request.distro,
        issuer: request.issuer.clone(),
        internal,
        audiences: request.audiences.clone(),
        max_token_lifetime: request.max_token_lifetime,
        max_elevated_token_lifetime: request.max_elevated_token_lifetime,
    })?;
    Ok(Prepared {
        configuration,
        issued_until,
    })
}

async fn ensure_certificate(
    client: Client,
    request: &Request,
) -> Result<(InternalDiscovery, Option<UnixTime>), KubeConfigError> {
    let namespace = &request.namespace;
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let no_service = || KubeConfigError::NoService {
        namespace: namespace.clone(),
        name: NAME,
    };
    let cluster_ip = services
        .get_opt(NAME)
        .await?
        .and_then(|s| s.spec)
        .and_then(|spec| spec.cluster_ip)
        .filter(|ip| !ip.is_empty() && ip != "None")
        .ok_or_else(no_service)?;
    let ip: IpAddr = cluster_ip
        .parse()
        .map_err(|_parse| KubeConfigError::BadClusterIp(cluster_ip.clone()))?;
    let url = internal_tls::internal_url(ip)?;

    let secrets: Api<Secret> = Api::namespaced(client, namespace);
    let existing = secrets.get_opt(internal_tls::SECRET).await?;
    if let Some(secret) = &existing
        && !request.renew
        && let Ok(material) = internal_tls::parse(secret)
        && material.url.as_ref() == Some(&url)
        && material
            .not_after
            .is_none_or(|t| t > request.now.saturating_add(RENEW_BEFORE))
    {
        let ca_pem = material
            .ca_pem
            .ok_or(KubeConfigError::NoCa(internal_tls::SECRET))?;
        return Ok((InternalDiscovery { url, ca_pem }, None));
    }

    let dns_names = [
        format!("{NAME}.{namespace}.svc"),
        format!("{NAME}.{namespace}.svc.cluster.local"),
    ];
    let issued =
        genkan_tls::certificate::issue(ip, &dns_names, request.now, request.certificate_validity)
            .map_err(TlsError::from)?;
    let mut secret = internal_tls::secret(namespace, &issued, &url);
    let params = PostParams::default();
    match existing {
        // Replace with the version read above: a concurrent change makes this fail, not race.
        Some(current) => {
            secret.metadata.resource_version = current.resource_version();
            secrets
                .replace(internal_tls::SECRET, &params, &secret)
                .await?;
        }
        None => {
            secrets.create(&params, &secret).await?;
        }
    }
    Ok((
        InternalDiscovery {
            url,
            ca_pem: issued.ca_pem,
        },
        Some(issued.not_after),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(distro: Distro, internal: bool) -> ApiserverOptions {
        ApiserverOptions {
            distro,
            issuer: BaseUrl::parse("issuer", "https://login.example.com").unwrap(),
            internal: internal.then(|| InternalDiscovery {
                url: BaseUrl::parse("internal", "https://10.43.0.53").unwrap(),
                ca_pem: "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n".into(),
            }),
            audiences: vec!["kubernetes".into()],
            max_token_lifetime: Duration::from_hours(1),
            max_elevated_token_lifetime: Duration::from_mins(5),
        }
    }

    #[test]
    fn k3s_internal_configuration() {
        let yaml = authentication_configuration(&options(Distro::K3s, true)).unwrap();
        let expected = r#"# kube-apiserver AuthenticationConfiguration for Genkan, from `genkanctl kube-config`.
apiVersion: apiserver.config.k8s.io/v1
kind: AuthenticationConfiguration
anonymous:
  enabled: false
jwt:
- issuer:
    url: "https://login.example.com"
    discoveryURL: "https://10.43.0.53/.well-known/openid-configuration"
    certificateAuthority: |
      -----BEGIN CERTIFICATE-----
      AAAA
      -----END CERTIFICATE-----
    audiences:
    - "kubernetes"
    audienceMatchPolicy: MatchAny
  claimValidationRules:
  - expression: "has(claims.client_id)"
    message: "only Genkan access tokens are accepted (ID tokens carry no client_id)"
  - expression: "claims.exp - claims.iat <= 3600"
    message: "token lifetime must not exceed 3600 seconds"
  - expression: "!has(claims.groups) || !dyn(claims.groups).exists(g, g.startsWith('elevated:')) || (claims.exp - claims.iat <= 300 && claims.iat - claims.auth_time <= 120)"
    message: "elevated groups need a token of at most 300 seconds issued right after a passkey sign-in"
  claimMappings:
    username:
      claim: preferred_username
      prefix: "genkan:"
    groups:
      claim: groups
      prefix: "genkan:"
    uid:
      claim: sub
    extra:
    - key: "genkan.engawahq.dev/client-id"
      valueExpression: "claims.client_id"
    - key: "genkan.engawahq.dev/amr"
      valueExpression: "has(claims.amr) ? claims.amr : []"
    - key: "genkan.engawahq.dev/jti"
      valueExpression: "claims.jti"
  userValidationRules:
  - expression: "!user.username.startsWith('system:')"
    message: "the system: prefix is reserved"
  - expression: "user.groups.all(g, !g.startsWith('system:'))"
    message: "the system: prefix is reserved"
"#;
        assert_eq!(yaml, expected);
    }

    #[test]
    fn anonymous_section_follows_the_distribution() {
        let kubeadm = authentication_configuration(&options(Distro::Kubeadm, true)).unwrap();
        assert!(kubeadm.contains("  enabled: true\n  conditions:\n  - path: /livez\n"));
        for distro in [Distro::K0s, Distro::Generic] {
            let yaml = authentication_configuration(&options(distro, true)).unwrap();
            assert!(!yaml.contains("anonymous"), "{distro:?}");
        }
        assert!(matches!(
            authentication_configuration(&options(Distro::Kubesolo, true)),
            Err(KubeConfigError::Unsupported)
        ));
    }

    #[test]
    fn public_discovery_has_no_discovery_url_or_ca() {
        let yaml = authentication_configuration(&options(Distro::K3s, false)).unwrap();
        assert!(!yaml.contains("discoveryURL") && !yaml.contains("certificateAuthority"));
    }

    #[test]
    fn audiences_are_validated() {
        for bad in ["", "two words", "tab\t"] {
            let mut o = options(Distro::K3s, false);
            o.audiences = vec![bad.into()];
            assert!(matches!(
                authentication_configuration(&o),
                Err(KubeConfigError::BadAudience(_))
            ));
        }
    }

    #[test]
    fn instructions_name_the_file_and_the_restart() {
        let k3s = instructions(Distro::K3s, DiscoveryMode::Internal);
        assert!(
            k3s.contains("/etc/rancher/k3s/genkan/authn.yaml")
                && k3s.contains("systemctl restart k3s")
        );
        let k0s = instructions(Distro::K0s, DiscoveryMode::Internal);
        assert!(k0s.contains("0644") && k0s.contains("oidc-*"));
        let kubeadm = instructions(Distro::Kubeadm, DiscoveryMode::Public);
        assert!(kubeadm.contains("extraVolumes") && kubeadm.contains("Public discovery"));
    }
}
