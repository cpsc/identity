//! SCIM 2.0 provisioning through the HTTP router, with in-memory users: what an identity
//! provider does (create, find, deactivate, group membership, delete) and the boundaries
//! (bearer token, users SCIM did not create, allowlisted groups only).

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use genkan_core::crypto::hash_secret;
    use genkan_scim::Scim;
    use genkan_store::scim::{MemoryUserAdmin, UserAdmin as _};
    use http_body_util::BodyExt as _;
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    const TOKEN: &str = "scim-token-with-at-least-thirty-two-characters";
    const BASE: &str = "https://login.example.com/scim/v2";
    const PATCH: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

    fn app() -> (Router, Arc<Scim<MemoryUserAdmin>>) {
        let users = MemoryUserAdmin::default();
        users.put_other("admin");
        let state = Arc::new(Scim {
            users,
            token_hash: hash_secret(TOKEN.as_bytes()),
            groups: vec!["dev".into(), "ops".into()],
            base: BASE.into(),
        });
        (genkan_scim::router(Arc::clone(&state)), state)
    }

    async fn call(
        app: &Router,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value, axum::http::HeaderMap) {
        let mut request = Request::builder()
            .method(method)
            .uri(format!("/scim/v2{path}"));
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/scim+json");
        }
        let request = request
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = body.collect().await.unwrap().to_bytes();
        (
            parts.status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            parts.headers,
        )
    }

    async fn scim(
        app: &Router,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, body, _) = call(app, method, path, body, Some(TOKEN)).await;
        (status, body)
    }

    #[tokio::test]
    async fn requires_the_bearer_token() {
        let (app, _) = app();
        for token in [None, Some("wrong-token")] {
            let (status, body, _) = call(&app, Method::GET, "/Users", None, token).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(
                body["schemas"][0],
                "urn:ietf:params:scim:api:messages:2.0:Error"
            );
        }
        let (status, body, headers) = call(
            &app,
            Method::GET,
            "/ServiceProviderConfig",
            None,
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/scim+json");
        assert_eq!(body["patch"]["supported"], true);
        assert_eq!(body["bulk"]["supported"], false);
        assert_eq!(
            scim(&app, Method::GET, "/ResourceTypes", None).await.1["totalResults"],
            2
        );
        assert_eq!(
            scim(&app, Method::GET, "/Schemas", None).await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    #[expect(
        clippy::too_many_lines,
        reason = "one provisioning scenario reads best top to bottom"
    )]
    async fn user_lifecycle() {
        let (app, state) = app();
        let (status, body, headers) = call(
            &app,
            Method::POST,
            "/Users",
            Some(json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
                "userName": "Alice@Example.com",
                "externalId": "00u1",
                "name": { "formatted": "Alice Example" },
                "emails": [{ "value": "alice@example.com", "primary": true }],
                "active": true,
            })),
            Some(TOKEN),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = body["id"].as_str().unwrap().to_owned();
        assert!(id.starts_with("u-"));
        assert_eq!(body["userName"], "alice@example.com");
        assert_eq!(body["displayName"], "Alice Example");
        assert_eq!(body["meta"]["location"], format!("{BASE}/Users/{id}"));
        assert_eq!(headers[header::LOCATION], format!("{BASE}/Users/{id}"));

        // userName is unique, also against users SCIM did not create; and must be valid.
        for (user_name, expected) in [
            ("alice@example.com", StatusCode::CONFLICT),
            ("admin", StatusCode::CONFLICT),
            ("not valid!", StatusCode::BAD_REQUEST),
        ] {
            let (status, _) = scim(
                &app,
                Method::POST,
                "/Users",
                Some(json!({ "userName": user_name })),
            )
            .await;
            assert_eq!(status, expected, "{user_name}");
        }

        // Find by filter, as identity providers do before creating.
        let (_, list) = scim(
            &app,
            Method::GET,
            "/Users?filter=userName%20eq%20%22ALICE%40example.com%22",
            None,
        )
        .await;
        assert_eq!(list["totalResults"], 1);
        assert_eq!(list["Resources"][0]["id"], id.as_str());
        let (_, list) = scim(
            &app,
            Method::GET,
            "/Users?filter=userName%20eq%20%22admin%22",
            None,
        )
        .await;
        assert_eq!(
            list["totalResults"], 0,
            "users SCIM did not create are invisible"
        );

        // Deactivate the way Microsoft Entra ID does.
        let (status, body) = scim(
            &app,
            Method::PATCH,
            &format!("/Users/{id}"),
            Some(json!({ "schemas": [PATCH], "Operations": [{ "op": "Replace", "path": "active", "value": "False" }] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["active"], false);
        assert!(state.users.get(&id).await.unwrap().unwrap().disabled);

        // PUT replaces; userName cannot change.
        let (status, body) = scim(
            &app,
            Method::PUT,
            &format!("/Users/{id}"),
            Some(json!({ "userName": "alice@example.com", "displayName": "A.", "active": true })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["displayName"], "A.");
        let (status, body) = scim(
            &app,
            Method::PUT,
            &format!("/Users/{id}"),
            Some(json!({ "userName": "mallory@example.com" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["scimType"], "mutability");

        assert_eq!(
            scim(&app, Method::DELETE, &format!("/Users/{id}"), None)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            scim(&app, Method::GET, &format!("/Users/{id}"), None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            scim(&app, Method::DELETE, &format!("/Users/{id}"), None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn groups_are_allowlisted_and_map_to_user_groups() {
        let (app, state) = app();
        let mut ids = Vec::new();
        for name in ["bob", "carol"] {
            let (_, body) = scim(
                &app,
                Method::POST,
                "/Users",
                Some(json!({ "userName": name })),
            )
            .await;
            ids.push(body["id"].as_str().unwrap().to_owned());
        }

        let (_, list) = scim(&app, Method::GET, "/Groups", None).await;
        assert_eq!(list["totalResults"], 2);
        let (status, body) = scim(
            &app,
            Method::POST,
            "/Groups",
            Some(json!({ "displayName": "admins" })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "not allowlisted: {body}");
        assert_eq!(
            scim(&app, Method::GET, "/Groups/admins", None).await.0,
            StatusCode::NOT_FOUND
        );

        let (status, body) = scim(
            &app,
            Method::POST,
            "/Groups",
            Some(json!({ "displayName": "dev", "members": [{ "value": ids[0] }] })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["members"][0]["value"], ids[0].as_str());
        assert_eq!(
            state.users.get(&ids[0]).await.unwrap().unwrap().groups,
            ["dev"]
        );

        let (status, body) = scim(
            &app,
            Method::PATCH,
            "/Groups/dev",
            Some(json!({ "schemas": [PATCH], "Operations": [
                { "op": "add", "path": "members", "value": [{ "value": ids[1] }] },
                { "op": "remove", "path": format!("members[value eq \"{}\"]", ids[0]) },
            ]})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            state.users.get(&ids[0]).await.unwrap().unwrap().groups,
            Vec::<String>::new()
        );
        assert_eq!(
            state.users.get(&ids[1]).await.unwrap().unwrap().groups,
            ["dev"]
        );
        // The user resource shows its groups.
        let (_, user) = scim(&app, Method::GET, &format!("/Users/{}", ids[1]), None).await;
        assert_eq!(user["groups"][0]["value"], "dev");

        let (status, _) = scim(
            &app,
            Method::PATCH,
            "/Groups/dev",
            Some(json!({ "schemas": [PATCH], "Operations": [{ "op": "add", "path": "members", "value": [{ "value": "u-nobody" }] }]})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        assert_eq!(
            scim(&app, Method::DELETE, "/Groups/dev", None).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            state.users.get(&ids[1]).await.unwrap().unwrap().groups,
            Vec::<String>::new()
        );
    }
}
