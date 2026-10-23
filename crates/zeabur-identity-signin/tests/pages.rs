//! The sign-in pages through the HTTP router: security headers, languages, markup the CSP allows,
//! passkey autofill, and the HTML error page for untrusted authorization requests.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, Request, StatusCode, header};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;
    use zeabur_identity_core::clock::SystemClock;
    use zeabur_identity_core::config::BaseUrl;
    use zeabur_identity_core::crypto::Sealer;
    use zeabur_identity_keys::Algorithm;
    use zeabur_identity_keys::manager::KeyManager;
    use zeabur_identity_keys::rotation::RotationPolicy;
    use zeabur_identity_keys::store::MemoryKeyStore;
    use zeabur_identity_oauth::endpoints::{self, FLOW_COOKIE};
    use zeabur_identity_oauth::{AuthorizationServer, Lifetimes, OneTimeIds};
    use zeabur_identity_signin::pages;
    use zeabur_identity_signin::upstream::provider::Kind;
    use zeabur_identity_store::{Client, ClientType, MemoryDirectory, MemoryGrantStore};
    use zeabur_identity_ui::CSP;

    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const HOME: &str = "https://dashboard.example.com/";

    async fn app() -> Router {
        app_with(&[]).await
    }

    async fn app_with(providers: &[Kind]) -> Router {
        let manager = KeyManager::new(
            MemoryKeyStore::default(),
            SystemClock,
            RotationPolicy::DEFAULT,
        );
        manager.reconcile().await.unwrap();
        let directory = MemoryDirectory::default();
        directory.put_client(Client {
            id: "dashboard".into(),
            display_name: Some("Cluster Dashboard".into()),
            kind: ClientType::Public,
            redirect_uris: vec!["https://dashboard.example.com/cb".into()],
            audiences: vec![],
            secret_hash: None,
            id_token_alg: Algorithm::Es256,
            allow_elevation: true,
            home_url: Some(HOME.into()),
        });
        let server = Arc::new(AuthorizationServer {
            issuer: BaseUrl::parse("issuer", "https://login.example.com").unwrap(),
            lifetimes: Lifetimes::DEFAULT,
            directory,
            grants: MemoryGrantStore::default(),
            clock: SystemClock,
            keys: manager.subscribe(),
            sealer: Sealer::ephemeral().unwrap(),
            used_codes: OneTimeIds::default(),
            used_request_uris: OneTimeIds::default(),
        });
        endpoints::router(Arc::clone(&server)).merge(pages::router(server, providers))
    }

    struct Page {
        status: StatusCode,
        headers: HeaderMap,
        body: String,
    }

    async fn get(app: &Router, uri: &str, headers: &[(&str, &str)]) -> Page {
        let mut request = Request::get(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Page {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// Everything a page must satisfy regardless of content.
    fn assert_hardened(page: &Page) {
        let header = |name: &str| page.headers[name].to_str().unwrap().to_owned();
        assert_eq!(header("content-security-policy"), CSP);
        assert_eq!(header("x-frame-options"), "DENY");
        assert_eq!(header("x-content-type-options"), "nosniff");
        assert_eq!(header("cache-control"), "no-store");
        assert_eq!(header("referrer-policy"), "no-referrer");
        assert!(header("content-type").starts_with("text/html"));
        assert!(CSP.contains("frame-ancestors 'none'") && !CSP.contains("unsafe"));

        // The CSP would block these anyway; failing here catches the mistake before a browser does.
        let html = &page.body;
        for (index, _) in html.match_indices("<script") {
            let tag = html.get(index..).unwrap().split('>').next().unwrap();
            assert!(tag.contains(" src=\"/assets/"), "inline script: {tag}");
        }
        for (index, _) in html.match_indices("<link") {
            let tag = html.get(index..).unwrap().split('>').next().unwrap();
            assert!(tag.contains(" href=\"/assets/"), "external resource: {tag}");
        }
        assert!(!html.contains("<style"), "inline style element");
        assert!(!html.contains(" style=\""), "inline style attribute");
        assert!(!html.contains("javascript:"));
        for attribute in [
            " onclick=",
            " onload=",
            " onerror=",
            " onsubmit=",
            " oninput=",
        ] {
            assert!(!html.contains(attribute), "inline handler {attribute}");
        }
        // Nothing loads from, links to, or submits to another origin, except a registered
        // application's home URL. (An SVG namespace is a name, never fetched.)
        let html = html
            .replace("xmlns=\"http://www.w3.org/2000/svg\"", "")
            .replace(&format!("<a href=\"{HOME}\""), "");
        assert!(
            !html.contains("http://") && !html.contains("https://"),
            "absolute URL in page"
        );
    }

    #[tokio::test]
    async fn login_without_a_request_explains_itself() {
        let app = app().await;
        let page = get(&app, "/login", &[]).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_hardened(&page);
        assert!(page.body.contains("<html lang=\"en\""));
        assert!(page.body.contains("Nothing to sign in to"));
        assert!(!page.body.contains("id=\"sign-in\""));
        // Clients with a home URL are offered by their display name.
        assert!(page.body.contains(&format!("<a href=\"{HOME}\"")));
        assert!(page.body.contains("Open Cluster Dashboard"));
        assert!(!page.body.contains("Lost all your passkeys?"));
    }

    #[tokio::test]
    async fn login_offers_passkey_autofill_for_a_pending_request() {
        let app = app().await;
        let authorize = get(
            &app,
            &format!(
                "/authorize?response_type=code&client_id=dashboard&redirect_uri=https%3A%2F%2Fdashboard.example.com%2Fcb\
                 &scope=openid&code_challenge={CHALLENGE}&code_challenge_method=S256&login_hint=alice"
            ),
            &[],
        )
        .await;
        assert_eq!(authorize.status, StatusCode::SEE_OTHER);
        let cookie = authorize.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(cookie.starts_with(FLOW_COOKIE));

        let page = get(&app, "/login", &[("cookie", &cookie)]).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_hardened(&page);
        assert!(page.body.contains("Cluster Dashboard"));
        assert!(page.body.contains("autocomplete=\"username webauthn\""));
        assert!(page.body.contains("value=\"alice\""));
        // With a login hint the username stays in view: nothing to fold.
        assert!(!page.body.contains("data-collapsible"));
        assert!(!page.body.contains("id=\"use-username\""));
        assert!(page.body.contains("Lost all your passkeys?"));
        assert!(
            page.body
                .replace("<span class=\"inline-block max-w-full\">", "")
                .replace("</span>", "")
                .contains("zeabur-identityctl setup-link --username &#60;name&#62; --reset")
        );
        assert!(!page.body.contains("Open Cluster Dashboard"));
        assert!(page.body.contains("<noscript>"));
        assert!(page.body.contains("id=\"unsupported\""));
        assert!(page.body.contains("type=\"module\""));
    }

    #[tokio::test]
    async fn user_controlled_text_is_escaped() {
        let app = app().await;
        let authorize = get(
            &app,
            &format!(
                "/authorize?response_type=code&client_id=dashboard&redirect_uri=https%3A%2F%2Fdashboard.example.com%2Fcb\
                 &scope=openid&code_challenge={CHALLENGE}&code_challenge_method=S256\
                 &login_hint=%22%3E%3Cscript%3Ealert(1)%3C%2Fscript%3E"
            ),
            &[],
        )
        .await;
        let cookie = authorize.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let page = get(&app, "/login", &[("cookie", &cookie)]).await;
        assert_hardened(&page);
        assert!(!page.body.contains("<script>alert"));
        assert!(
            page.body.contains("&#34;&#62;&#60;script&#62;")
                || page.body.contains("&quot;&gt;&lt;script&gt;")
        );
    }

    #[tokio::test]
    async fn language_follows_parameter_then_header() {
        let app = app().await;
        let page = get(
            &app,
            "/login",
            &[("accept-language", "fr, zh-Hant;q=0.9, en;q=0.8")],
        )
        .await;
        assert!(page.body.contains("<html lang=\"zh-TW\""));
        assert!(page.body.contains("目前沒有登入要求"));

        let page = get(&app, "/login?lang=zh-CN", &[("accept-language", "zh-TW")]).await;
        assert!(page.body.contains("<html lang=\"zh-CN\""));
        assert!(page.body.contains("当前没有登录请求"));

        let page = get(&app, "/login?lang=xx", &[("accept-language", "xx")]).await;
        assert!(page.body.contains("<html lang=\"en\""));
        // The switcher marks the current language and links to the others.
        assert!(page.body.contains("?lang=zh-TW"));
        assert!(page.body.contains("aria-current"));
    }

    /// A used or expired link leaves only the way to a new one: the script hides the form and the
    /// provider buttons and shows the new-link guidance, all found by these ids.
    #[tokio::test]
    async fn setup_page_has_what_a_used_link_hides() {
        let app = app_with(&[Kind::GitHub]).await;
        let setup = get(&app, "/assets/setup.js", &[]).await.body;
        let page = get(&app, "/setup", &[]).await;
        for id in ["setup", "upstream", "new-link"] {
            assert!(setup.contains(&format!("\"{id}\"")), "setup.js: {id}");
            assert!(page.body.contains(&format!("id=\"{id}\"")), "page: {id}");
        }
        assert!(setup.contains("\"setup-invalid\""));
        assert!(page.body.contains("<div id=\"new-link\" hidden>"));
        assert!(page.body.contains("data-msg-invalid="));
    }

    #[tokio::test]
    async fn setup_page_loads_its_script() {
        let app = app().await;
        let page = get(&app, "/setup", &[]).await;
        assert_eq!(page.status, StatusCode::OK);
        assert_hardened(&page);
        assert!(page.body.contains("/assets/setup.js?v="));
        assert!(page.body.contains("id=\"setup\""));
        // The token travels in the fragment; the page never asks for it in a form field.
        assert!(!page.body.contains("name=\"token\""));
    }

    #[tokio::test]
    async fn untrusted_authorization_requests_show_an_error_page() {
        let app = app().await;
        let page = get(
            &app,
            "/authorize?response_type=code&client_id=nobody&redirect_uri=https%3A%2F%2Fevil.example%2F&ui_locales=de%20zh-CN",
            &[("accept-language", "en")],
        )
        .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST);
        assert!(!page.headers.contains_key(header::LOCATION));
        assert_hardened(&page);
        assert!(page.body.contains("<html lang=\"zh-CN\""));
        assert!(page.body.contains("此应用尚未注册"));
        assert!(page.body.contains("<details"));
        assert!(
            page.body
                .contains("invalid_request: The client_id is missing or not registered.")
        );

        let page = get(
            &app,
            "/authorize?response_type=code&client_id=dashboard&redirect_uri=https%3A%2F%2Fevil.example%2F",
            &[("accept-language", "en")],
        )
        .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST);
        assert_hardened(&page);
        assert!(
            page.body
                .contains("This app’s return address isn’t registered")
        );
        assert!(!page.body.contains("evil.example"), "input is never echoed");

        let page = get(
            &app,
            "/authorize?client_id=dashboard&request_uri=urn%3Aietf%3Aparams%3Aoauth%3Arequest_uri%3Aforged",
            &[],
        )
        .await;
        assert_eq!(page.status, StatusCode::BAD_REQUEST);
        assert!(page.body.contains("This sign-in expired"));
    }

    #[tokio::test]
    async fn assets_are_embedded_versioned_and_immutable() {
        let app = app().await;
        let page = get(&app, "/login", &[]).await;
        let start = page.body.find("/assets/app.css?v=").unwrap();
        let href = page
            .body
            .get(start..)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned();
        let version = href.split("?v=").nth(1).unwrap();
        assert_eq!(version.len(), 16);

        for (path, kind) in [
            (href.as_str(), "text/css"),
            ("/assets/login.js", "text/javascript"),
            ("/assets/setup.js", "text/javascript"),
            ("/assets/Geist-Variable.woff2", "font/woff2"),
        ] {
            let asset = get(&app, path, &[]).await;
            assert_eq!(asset.status, StatusCode::OK, "{path}");
            assert!(
                asset.headers[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with(kind)
            );
            assert!(
                asset.headers[header::CACHE_CONTROL]
                    .to_str()
                    .unwrap()
                    .contains("immutable")
            );
            assert_eq!(asset.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        }
        for missing in ["/assets/nope.js", "/assets/..%2FCargo.toml", "/assets/"] {
            assert_ne!(
                get(&app, missing, &[]).await.status,
                StatusCode::OK,
                "{missing}"
            );
        }
    }

    #[tokio::test]
    async fn provider_buttons_appear_only_when_configured_and_not_for_elevation() {
        let plain = app().await;
        let page = get(&plain, "/setup", &[]).await;
        assert!(!page.body.contains("data-provider"));

        let app = app_with(&[Kind::GitHub, Kind::Google]).await;
        let page = get(&app, "/setup", &[]).await;
        assert_hardened(&page);
        assert!(page.body.contains(r#"data-provider="github""#));
        assert!(page.body.contains("Continue with Google"));

        let authorize = |scope: &str| {
            format!(
                "/authorize?response_type=code&client_id=dashboard&redirect_uri=https%3A%2F%2Fdashboard.example.com%2Fcb\
                 &scope={scope}&code_challenge={CHALLENGE}&code_challenge_method=S256"
            )
        };
        for (scope, offered) in [("openid", true), ("openid+elevate", false)] {
            let reply = get(&app, &authorize(scope), &[]).await;
            let cookie = reply.headers[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            let page = get(&app, "/login", &[("cookie", &cookie)]).await;
            assert_eq!(page.body.contains("data-provider"), offered, "{scope}");
        }
    }

    #[tokio::test]
    async fn login_says_when_elevated_access_is_requested() {
        let app = app().await;
        for (scope, lang, notice) in [
            ("openid", "en", None),
            (
                "openid+elevate",
                "en",
                Some(
                    "Cluster Dashboard is requesting elevated access: your admin rights on the cluster, such as changing or deleting what runs on it. It ends by itself after 5\u{a0}min.",
                ),
            ),
            (
                "openid+elevate",
                "zh-TW",
                Some("權限會在 5\u{a0}分鐘後自動失效。"),
            ),
        ] {
            let reply = get(
                &app,
                &format!(
                    "/authorize?response_type=code&client_id=dashboard&redirect_uri=https%3A%2F%2Fdashboard.example.com%2Fcb\
                     &scope={scope}&code_challenge={CHALLENGE}&code_challenge_method=S256"
                ),
                &[],
            )
            .await;
            let cookie = reply.headers[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned();
            let page = get(&app, &format!("/login?lang={lang}"), &[("cookie", &cookie)]).await;
            assert_hardened(&page);
            assert_eq!(
                page.body.contains("id=\"elevation\""),
                notice.is_some(),
                "{scope} {lang}"
            );
            if let Some(text) = notice {
                assert!(page.body.contains(text), "{scope} {lang}");
            }
        }
    }
}
