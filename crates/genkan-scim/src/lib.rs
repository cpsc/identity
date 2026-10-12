#![doc = include_str!("../README.md")]

pub mod model;

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use genkan_core::crypto::{ct_eq, hash_secret, hex, random};
use genkan_store::scim::{ManagedUser, UserAdmin, Write};
use serde_json::Value;

use self::model::{
    Filter, PatchRequest, ScimError, apply_group_patch, apply_user_patch, apply_user_resource,
    group_json, group_members, list_json, parse_filter, resource_types, respond, schemas,
    service_provider_config, user_json,
};

/// Where the SCIM endpoints live.
pub const BASE_PATH: &str = "/scim/v2";
/// Largest page returned.
const MAX_COUNT: usize = 200;
/// Attempts at a membership change that races with another write.
const RETRIES: usize = 3;

/// SCIM state shared by the handlers.
pub struct Scim<A> {
    /// The users SCIM manages.
    pub users: A,
    /// `sha256:<hex>` of the bearer token.
    pub token_hash: String,
    /// Groups SCIM may assign.
    pub groups: Vec<String>,
    /// Absolute base URL (`<issuer>/scim/v2`), for `location` and `$ref`.
    pub base: String,
}

impl<A> std::fmt::Debug for Scim<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scim")
            .field("groups", &self.groups)
            .finish_non_exhaustive()
    }
}

type Shared<A> = Arc<Scim<A>>;
type Result<T> = std::result::Result<T, ScimError>;

/// The SCIM routes.
pub fn router<A: UserAdmin>(scim: Shared<A>) -> Router {
    Router::new()
        .route(
            &format!("{BASE_PATH}/ServiceProviderConfig"),
            get(provider_config::<A>),
        )
        .route(&format!("{BASE_PATH}/ResourceTypes"), get(types::<A>))
        .route(&format!("{BASE_PATH}/Schemas"), get(schema_list::<A>))
        .route(
            &format!("{BASE_PATH}/Users"),
            get(list_users::<A>).post(create_user::<A>),
        )
        .route(
            &format!("{BASE_PATH}/Users/{{id}}"),
            get(get_user::<A>)
                .put(replace_user::<A>)
                .patch(patch_user::<A>)
                .delete(delete_user::<A>),
        )
        .route(
            &format!("{BASE_PATH}/Groups"),
            get(list_groups::<A>).post(create_group::<A>),
        )
        .route(
            &format!("{BASE_PATH}/Groups/{{id}}"),
            get(get_group::<A>)
                .put(replace_group::<A>)
                .patch(patch_group::<A>)
                .delete(delete_group::<A>),
        )
        .with_state(scim)
}

fn authorize<A>(scim: &Scim<A>, headers: &HeaderMap) -> Result<()> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim());
    let valid = token.is_some_and(|token| {
        ct_eq(
            hash_secret(token.as_bytes()).as_bytes(),
            scim.token_hash.as_bytes(),
        )
    });
    if valid {
        Ok(())
    } else {
        Err(ScimError {
            status: StatusCode::UNAUTHORIZED,
            scim_type: None,
            detail: "a valid bearer token is required".to_owned(),
        })
    }
}

fn store_error(error: impl std::fmt::Display) -> ScimError {
    tracing::error!(%error, "SCIM storage failed");
    ScimError::internal()
}

fn body_json(body: &Bytes) -> Result<Value> {
    serde_json::from_slice(body).map_err(|_json| ScimError::bad("invalidSyntax", "malformed JSON"))
}

fn finish(result: Result<Response>) -> Response {
    result.unwrap_or_else(IntoResponse::into_response)
}

/// `filter`, `startIndex` and `count` from the query.
fn list_params(query: Option<&str>) -> Result<(Option<Filter>, usize, usize)> {
    let mut filter = None;
    let mut start = 1;
    let mut count = MAX_COUNT;
    for (key, value) in form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "filter" => filter = Some(parse_filter(&value)?),
            "startIndex" => start = value.parse::<usize>().unwrap_or(1).max(1),
            "count" => count = value.parse::<usize>().unwrap_or(MAX_COUNT).min(MAX_COUNT),
            _ => {}
        }
    }
    Ok((filter, start, count))
}

/// A valid username from a SCIM `userName`: lowercase, the same pattern as the `User` schema.
fn username(value: &str) -> Result<String> {
    let lower = value.trim().to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let edge = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let inner = |b: &u8| edge(b) || b"-._@+".contains(b);
    let valid = !bytes.is_empty()
        && bytes.len() <= 128
        && bytes.first().is_some_and(edge)
        && bytes.last().is_some_and(edge)
        && bytes.iter().all(inner);
    if valid {
        Ok(lower)
    } else {
        Err(ScimError::bad(
            "invalidValue",
            "userName must be 1 to 128 characters: letters, digits and - . _ @ +",
        ))
    }
}

async fn provider_config<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    headers: HeaderMap,
) -> Response {
    finish(
        authorize(&scim, &headers)
            .map(|()| respond(StatusCode::OK, &service_provider_config(&scim.base))),
    )
}

async fn types<A: UserAdmin>(State(scim): State<Shared<A>>, headers: HeaderMap) -> Response {
    finish(
        authorize(&scim, &headers).map(|()| respond(StatusCode::OK, &resource_types(&scim.base))),
    )
}

async fn schema_list<A: UserAdmin>(State(scim): State<Shared<A>>, headers: HeaderMap) -> Response {
    finish(authorize(&scim, &headers).map(|()| respond(StatusCode::OK, &schemas())))
}

async fn list_users<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let (filter, start, count) = list_params(query.as_deref())?;
            let users = scim.users.list().await.map_err(store_error)?;
            let matching = users.iter().filter(|user| {
                filter.as_ref().is_none_or(|f| match f.attribute.as_str() {
                    "username" => user.username.eq_ignore_ascii_case(&f.value),
                    "externalid" => user.external_id.as_deref() == Some(f.value.as_str()),
                    "id" => user.name == f.value,
                    _ => false,
                })
            });
            let resources = matching.map(|u| user_json(&scim.base, u)).collect();
            Ok(respond(StatusCode::OK, &list_json(resources, start, count)))
        }
        .await,
    )
}

async fn create_user<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let body = body_json(&body)?;
            let requested = body
                .get("userName")
                .and_then(Value::as_str)
                .ok_or_else(|| ScimError::bad("invalidValue", "userName is required"))?;
            let username = username(requested)?;
            if scim
                .users
                .username_taken(&username)
                .await
                .map_err(store_error)?
            {
                return Err(ScimError::exists("userName is taken"));
            }
            let mut user = ManagedUser {
                name: format!(
                    "u-{}",
                    hex(&random::<8>().map_err(|_crypto| ScimError::internal())?)
                ),
                username,
                ..ManagedUser::default()
            };
            apply_user_resource(&mut user, &body)?;
            match scim.users.create(&user).await.map_err(store_error)? {
                Write::Done(created) => {
                    tracing::info!(user = %created.name, "SCIM created a user");
                    let json = user_json(&scim.base, &created);
                    let mut response = respond(StatusCode::CREATED, &json);
                    if let Ok(location) = json
                        .get("meta")
                        .and_then(|m| m.get("location"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .parse()
                    {
                        response.headers_mut().insert(header::LOCATION, location);
                    }
                    Ok(response)
                }
                Write::Exists | Write::Conflict => Err(ScimError::exists("the user exists")),
                Write::NotFound => Err(ScimError::internal()),
            }
        }
        .await,
    )
}

async fn load_user<A: UserAdmin>(scim: &Scim<A>, id: &str) -> Result<ManagedUser> {
    scim.users
        .get(id)
        .await
        .map_err(store_error)?
        .ok_or_else(ScimError::not_found)
}

async fn get_user<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let user = load_user(&scim, &id).await?;
            Ok(respond(StatusCode::OK, &user_json(&scim.base, &user)))
        }
        .await,
    )
}

/// Writes `user`, which the caller derived from the version it read.
async fn store_user<A: UserAdmin>(scim: &Scim<A>, user: &ManagedUser) -> Result<Response> {
    match scim.users.replace(user).await.map_err(store_error)? {
        Write::Done(stored) => Ok(respond(StatusCode::OK, &user_json(&scim.base, &stored))),
        Write::Conflict => Err(ScimError {
            status: StatusCode::CONFLICT,
            scim_type: None,
            detail: "the user changed meanwhile; retry".to_owned(),
        }),
        Write::NotFound | Write::Exists => Err(ScimError::not_found()),
    }
}

async fn replace_user<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let body = body_json(&body)?;
            let mut user = load_user(&scim, &id).await?;
            if let Some(requested) = body.get("userName").and_then(Value::as_str)
                && requested.to_ascii_lowercase() != user.username
            {
                return Err(ScimError::bad("mutability", "userName cannot change"));
            }
            apply_user_resource(&mut user, &body)?;
            store_user(&scim, &user).await
        }
        .await,
    )
}

async fn patch_user<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let request: PatchRequest = serde_json::from_slice(&body)
                .map_err(|_json| ScimError::bad("invalidSyntax", "malformed PATCH request"))?;
            let mut user = load_user(&scim, &id).await?;
            apply_user_patch(&mut user, &request)?;
            store_user(&scim, &user).await
        }
        .await,
    )
}

async fn delete_user<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            match scim.users.delete(&id).await.map_err(store_error)? {
                Write::Done(_) => {
                    tracing::info!(user = %id, "SCIM deleted a user");
                    Ok(StatusCode::NO_CONTENT.into_response())
                }
                Write::NotFound | Write::Conflict | Write::Exists => Err(ScimError::not_found()),
            }
        }
        .await,
    )
}

fn allowed<A>(scim: &Scim<A>, group: &str) -> Result<()> {
    if scim.groups.iter().any(|g| g == group) {
        Ok(())
    } else {
        Err(ScimError::not_found())
    }
}

/// Sets exactly `members` (user IDs) as the members of `group`, user by user, retrying writes
/// that race with another change.
async fn set_members<A: UserAdmin>(scim: &Scim<A>, group: &str, members: &[String]) -> Result<()> {
    let users = scim.users.list().await.map_err(store_error)?;
    if let Some(unknown) = members
        .iter()
        .find(|m| !users.iter().any(|u| u.name == **m))
    {
        return Err(ScimError::bad(
            "invalidValue",
            format!("no such user: {unknown}"),
        ));
    }
    for user in users {
        let wanted = members.contains(&user.name);
        let mut current = user;
        for attempt in 0..RETRIES {
            if current.groups.iter().any(|g| g == group) == wanted {
                break;
            }
            let mut next = current.clone();
            if wanted {
                next.groups.push(group.to_owned());
            } else {
                next.groups.retain(|g| g != group);
            }
            match scim.users.replace(&next).await.map_err(store_error)? {
                Write::Done(_) | Write::NotFound => break,
                Write::Conflict | Write::Exists if attempt + 1 < RETRIES => {
                    match scim.users.get(&next.name).await.map_err(store_error)? {
                        Some(fresh) => current = fresh,
                        None => break,
                    }
                }
                Write::Conflict | Write::Exists => {
                    return Err(ScimError {
                        status: StatusCode::CONFLICT,
                        scim_type: None,
                        detail: "a member changed meanwhile; retry".to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

async fn members_of<A: UserAdmin>(scim: &Scim<A>, group: &str) -> Result<Vec<ManagedUser>> {
    let users = scim.users.list().await.map_err(store_error)?;
    Ok(users
        .into_iter()
        .filter(|u| u.groups.iter().any(|g| g == group))
        .collect())
}

async fn group_response<A: UserAdmin>(
    scim: &Scim<A>,
    group: &str,
    status: StatusCode,
) -> Result<Response> {
    let members = members_of(scim, group).await?;
    Ok(respond(status, &group_json(&scim.base, group, &members)))
}

async fn list_groups<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let (filter, start, count) = list_params(query.as_deref())?;
            let users = scim.users.list().await.map_err(store_error)?;
            let resources = scim
                .groups
                .iter()
                .filter(|group| {
                    filter.as_ref().is_none_or(|f| {
                        matches!(f.attribute.as_str(), "displayname" | "id") && f.value == **group
                    })
                })
                .map(|group| {
                    let members: Vec<ManagedUser> = users
                        .iter()
                        .filter(|u| u.groups.contains(group))
                        .cloned()
                        .collect();
                    group_json(&scim.base, group, &members)
                })
                .collect();
            Ok(respond(StatusCode::OK, &list_json(resources, start, count)))
        }
        .await,
    )
}

async fn create_group<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            let body = body_json(&body)?;
            let group = body
                .get("displayName")
                .and_then(Value::as_str)
                .ok_or_else(|| ScimError::bad("invalidValue", "displayName is required"))?
                .to_owned();
            if !scim.groups.contains(&group) {
                return Err(ScimError::bad(
                    "invalidValue",
                    "this group is not in Genkan's SCIM allowlist",
                ));
            }
            if !members_of(&scim, &group).await?.is_empty() {
                return Err(ScimError::exists("the group exists"));
            }
            set_members(&scim, &group, &group_members(&body)?).await?;
            group_response(&scim, &group, StatusCode::CREATED).await
        }
        .await,
    )
}

async fn get_group<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            allowed(&scim, &id)?;
            group_response(&scim, &id, StatusCode::OK).await
        }
        .await,
    )
}

async fn replace_group<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            allowed(&scim, &id)?;
            let body = body_json(&body)?;
            if body
                .get("displayName")
                .and_then(Value::as_str)
                .is_some_and(|name| name != id)
            {
                return Err(ScimError::bad("mutability", "a group cannot be renamed"));
            }
            set_members(&scim, &id, &group_members(&body)?).await?;
            group_response(&scim, &id, StatusCode::OK).await
        }
        .await,
    )
}

async fn patch_group<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            allowed(&scim, &id)?;
            let request: PatchRequest = serde_json::from_slice(&body)
                .map_err(|_json| ScimError::bad("invalidSyntax", "malformed PATCH request"))?;
            let current: Vec<String> = members_of(&scim, &id)
                .await?
                .into_iter()
                .map(|u| u.name)
                .collect();
            let members = apply_group_patch(current, &request)?;
            set_members(&scim, &id, &members).await?;
            group_response(&scim, &id, StatusCode::OK).await
        }
        .await,
    )
}

async fn delete_group<A: UserAdmin>(
    State(scim): State<Shared<A>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    finish(
        async {
            authorize(&scim, &headers)?;
            allowed(&scim, &id)?;
            set_members(&scim, &id, &[]).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        .await,
    )
}
