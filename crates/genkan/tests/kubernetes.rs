//! The server's background work against a real cluster: mailing invitation links.
//!
//! Needs a cluster in `KUBECONFIG` where the caller is cluster admin. Run:
//! `cargo nextest run --run-ignored only -E 'binary(kubernetes)'`

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use genkan_core::clock::{Clock as _, SystemClock};
    use genkan_k8s::crd::definitions;
    use genkan_k8s::rbac;
    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::{Api, DeleteParams, DynamicObject, Patch, PatchParams};
    use kube::core::GroupVersionKind;
    use kube::discovery::{Scope, pinned_kind};
    use kube::{Client, Config, ResourceExt as _};
    use tokio::sync::watch;

    /// A namespace with Genkan's CRDs and RBAC, and a client acting as Genkan's service account.
    struct Env {
        admin: Client,
        genkan: Client,
        namespace: String,
    }

    async fn apply(client: &Client, value: serde_json::Value) {
        let object: DynamicObject = serde_json::from_value(value).unwrap();
        let gvk = GroupVersionKind::try_from(object.types.as_ref().unwrap()).unwrap();
        let (resource, capabilities) = pinned_kind(client, &gvk).await.unwrap();
        let api: Api<DynamicObject> = match capabilities.scope {
            Scope::Namespaced => Api::namespaced_with(
                client.clone(),
                object.namespace().as_deref().unwrap(),
                &resource,
            ),
            Scope::Cluster => Api::all_with(client.clone(), &resource),
        };
        api.patch(
            &object.name_any(),
            &PatchParams::apply("genkan-test").force(),
            &Patch::Apply(&object),
        )
        .await
        .unwrap();
    }

    impl Env {
        async fn new() -> Self {
            let admin = Client::try_default().await.unwrap();
            for crd in definitions() {
                apply(&admin, serde_json::to_value(crd).unwrap()).await;
            }
            let namespace = format!("genkan-srv-{}", std::process::id());
            for object in rbac::manifests(&namespace).unwrap() {
                apply(&admin, object).await;
            }
            let mut config = Config::infer().await.unwrap();
            config.auth_info.impersonate =
                Some(format!("system:serviceaccount:{namespace}:{}", rbac::NAME));
            // CRDs may take a moment to be served.
            tokio::time::sleep(Duration::from_secs(2)).await;
            Self {
                admin,
                genkan: Client::try_from(config).unwrap(),
                namespace,
            }
        }

        fn admin_api<K>(&self) -> Api<K>
        where
            K: kube::Resource<Scope = kube::core::NamespaceResourceScope, DynamicType = ()>,
        {
            Api::namespaced(self.admin.clone(), &self.namespace)
        }

        fn genkan_api<K>(&self) -> Api<K>
        where
            K: kube::Resource<Scope = kube::core::NamespaceResourceScope, DynamicType = ()>,
        {
            Api::namespaced(self.genkan.clone(), &self.namespace)
        }

        async fn cleanup(self) {
            Api::<Namespace>::all(self.admin)
                .delete(&self.namespace, &DeleteParams::background())
                .await
                .unwrap();
        }
    }

    fn now() -> u64 {
        SystemClock.now().as_secs()
    }

    /// Accepts one SMTPS session and returns everything the client sent.
    async fn fake_smtps(
        listener: tokio::net::TcpListener,
        acceptor: tokio_rustls::TlsAcceptor,
    ) -> String {
        use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};
        let (tcp, _) = listener.accept().await.unwrap();
        let mut session = tokio::io::BufReader::new(acceptor.accept(tcp).await.unwrap());
        session.get_mut().write_all(b"220 fake\r\n").await.unwrap();
        let mut transcript = String::new();
        loop {
            let mut line = String::new();
            if session.read_line(&mut line).await.unwrap() == 0 {
                return transcript;
            }
            transcript.push_str(&line);
            let answer: &[u8] = match line.split_whitespace().next().unwrap_or_default() {
                "DATA" => {
                    session.get_mut().write_all(b"354 go\r\n").await.unwrap();
                    let mut data = Vec::new();
                    while !data.ends_with(b"\r\n.\r\n") {
                        let mut byte = [0_u8; 1];
                        session.read_exact(&mut byte).await.unwrap();
                        data.push(byte[0]);
                    }
                    transcript.push_str(&String::from_utf8_lossy(&data));
                    b"250 queued\r\n"
                }
                "QUIT" => {
                    drop(session.get_mut().write_all(b"221 bye\r\n").await);
                    return transcript;
                }
                _ => b"250 ok\r\n",
            };
            session.get_mut().write_all(answer).await.unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "needs a Kubernetes cluster (KUBECONFIG)"]
    #[expect(
        clippy::too_many_lines,
        reason = "one end-to-end scenario reads best top to bottom"
    )]
    async fn mailed_invitations_are_claimed_once_and_sent() {
        use base64::Engine as _;
        use genkan::mailer::run_mailer;
        use genkan_k8s::crd::invitation::Invitation;
        use genkan_k8s::invitations::{Invitee, invite};
        use genkan_mail::Mailer;
        use genkan_tls::certificate::{certified_key, issue};
        use genkan_tls::listener::{Resolver, server_config};
        use rustls::pki_types::CertificateDer;
        use rustls::pki_types::pem::PemObject as _;

        let env = Env::new().await;
        let issuer =
            genkan_core::config::BaseUrl::parse("issuer", "https://login.example.com").unwrap();
        let now = genkan_core::clock::UnixTime::from_secs(now());

        // A TLS mail server on localhost with its own CA.
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let mail_ca = issue(ip, &["localhost".into()], now, Duration::from_hours(1)).unwrap();
        let resolver = Arc::new(Resolver::default());
        resolver
            .set(certified_key(mail_ca.cert_pem.as_bytes(), mail_ca.key_pem.as_bytes()).unwrap());
        let mut config = (*server_config(resolver).unwrap()).clone();
        config.alpn_protocols.clear();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(fake_smtps(listener, acceptor));
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(mail_ca.ca_pem.as_bytes()) {
            roots.add(cert.unwrap()).unwrap();
        }
        let mailer = Mailer::new(
            &format!("smtps://localhost:{port}"),
            None,
            "genkan@example.com",
            roots,
        )
        .unwrap();

        let invited = invite(
            env.admin.clone(),
            &env.namespace,
            &issuer,
            &Invitee {
                username: "mallory".into(),
                display_name: None,
                groups: vec![],
                elevated_groups: vec![],
                email: Some("mallory@example.com".into()),
                language: Some("zh-CN".into()),
            },
            Duration::from_hours(24),
            now,
        )
        .await
        .unwrap();

        let (stop, shutdown) = watch::channel(false);
        let mailer_task = tokio::spawn(run_mailer(
            env.genkan_api(),
            mailer,
            issuer,
            SystemClock,
            shutdown,
        ));
        let transcript = tokio::time::timeout(Duration::from_secs(60), server)
            .await
            .unwrap()
            .unwrap();
        assert!(transcript.contains("RCPT TO:<mallory@example.com>"));
        let body: String = transcript
            .split("\r\n\r\n")
            .nth(1)
            .unwrap()
            .split("\r\n")
            .take_while(|l| *l != ".")
            .collect();
        let body = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(body)
                .unwrap(),
        )
        .unwrap();
        assert!(body.contains("通行密钥"), "{body}");
        let token = body
            .split("#token=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned();
        assert!(token.starts_with(&format!("{}.", invited.name)));

        // The hash of exactly that token is recorded, with the time it was sent.
        let invitations: Api<Invitation> = env.admin_api();
        let mut status = None;
        for _ in 0..50 {
            status = invitations.get(&invited.name).await.unwrap().status;
            if status.as_ref().is_some_and(|s| s.sent_at.is_some()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let status = status.unwrap();
        assert_eq!(
            status.token_hash.as_deref(),
            Some(genkan_core::crypto::hash_secret(token.as_bytes()).as_str())
        );
        assert!(status.sent_at.is_some());
        stop.send_replace(true);
        mailer_task.await.unwrap();
        env.cleanup().await;
    }
}
