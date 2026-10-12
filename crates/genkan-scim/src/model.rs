//! SCIM 2.0 resources (RFC 7643) and the parts of the protocol (RFC 7644) Genkan implements:
//! JSON for `User` and `Group`, list responses, errors, `eq` filters and PATCH operations.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use genkan_store::scim::ManagedUser;
use serde::Deserialize;
use serde_json::{Value, json};

/// RFC 7643 §8.7.1 schema URIs.
pub const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
/// Group schema.
pub const GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
const LIST_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const PATCH_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";

/// The SCIM media type (RFC 7644 §8.1).
pub const MEDIA_TYPE: &str = "application/scim+json";

/// A SCIM error response (RFC 7644 §3.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimError {
    /// HTTP status.
    pub status: StatusCode,
    /// `scimType`, for 400 and 409.
    pub scim_type: Option<&'static str>,
    /// Human-readable detail.
    pub detail: String,
}

impl ScimError {
    /// A 400 with `scimType`.
    #[must_use]
    pub fn bad(scim_type: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            scim_type: Some(scim_type),
            detail: detail.into(),
        }
    }

    /// A 404.
    #[must_use]
    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            scim_type: None,
            detail: "resource not found".to_owned(),
        }
    }

    /// A 409 `uniqueness`.
    #[must_use]
    pub fn exists(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            scim_type: Some("uniqueness"),
            detail: detail.into(),
        }
    }

    /// A 500.
    #[must_use]
    pub fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            scim_type: None,
            detail: "internal error".to_owned(),
        }
    }
}

/// A SCIM JSON response.
#[must_use]
pub fn respond(status: StatusCode, body: &Value) -> Response {
    let mut response = (status, body.to_string()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(MEDIA_TYPE),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

impl IntoResponse for ScimError {
    fn into_response(self) -> Response {
        let mut body = json!({
            "schemas": [ERROR_SCHEMA],
            "status": self.status.as_u16().to_string(),
            "detail": self.detail,
        });
        if let (Some(scim_type), Some(object)) = (self.scim_type, body.as_object_mut()) {
            object.insert("scimType".to_owned(), json!(scim_type));
        }
        respond(self.status, &body)
    }
}

/// The JSON of a user.
#[must_use]
pub fn user_json(base: &str, user: &ManagedUser) -> Value {
    let mut meta = json!({
        "resourceType": "User",
        "location": format!("{base}/Users/{}", user.name),
        "version": format!("W/\"{}\"", user.version),
    });
    if let (Some(created), Some(object)) = (&user.created, meta.as_object_mut()) {
        object.insert("created".to_owned(), json!(created));
    }
    let mut body = json!({
        "schemas": [USER_SCHEMA],
        "id": user.name,
        "userName": user.username,
        "active": !user.disabled,
        "groups": user.groups.iter().map(|g| json!({
            "value": g,
            "display": g,
            "$ref": format!("{base}/Groups/{g}"),
        })).collect::<Vec<_>>(),
        "meta": meta,
    });
    if let Some(object) = body.as_object_mut() {
        if let Some(display) = &user.display_name {
            object.insert("displayName".to_owned(), json!(display));
        }
        if let Some(id) = &user.external_id {
            object.insert("externalId".to_owned(), json!(id));
        }
    }
    body
}

/// The JSON of a group and its members.
#[must_use]
pub fn group_json(base: &str, group: &str, members: &[ManagedUser]) -> Value {
    json!({
        "schemas": [GROUP_SCHEMA],
        "id": group,
        "displayName": group,
        "members": members.iter().map(|u| json!({
            "value": u.name,
            "display": u.username,
            "$ref": format!("{base}/Users/{}", u.name),
        })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "Group",
            "location": format!("{base}/Groups/{group}"),
        },
    })
}

/// A list response (RFC 7644 §3.4.2), paginated with `startIndex` (1-based) and `count`.
#[must_use]
pub fn list_json(resources: Vec<Value>, start_index: usize, count: usize) -> Value {
    let total = resources.len();
    let page: Vec<Value> = resources
        .into_iter()
        .skip(start_index.saturating_sub(1))
        .take(count)
        .collect();
    json!({
        "schemas": [LIST_SCHEMA],
        "totalResults": total,
        "startIndex": start_index,
        "itemsPerPage": page.len(),
        "Resources": page,
    })
}

/// A parsed `filter`: only `<attribute> eq "<value>"` is supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// Attribute name, lowercased.
    pub attribute: String,
    /// The compared value.
    pub value: String,
}

/// Parses a filter (RFC 7644 §3.4.2.2), restricted to a single `eq` comparison.
///
/// # Errors
///
/// `invalidFilter` for anything else.
pub fn parse_filter(filter: &str) -> Result<Filter, ScimError> {
    let invalid = || {
        ScimError::bad(
            "invalidFilter",
            "only `<attribute> eq \"<value>\"` is supported",
        )
    };
    let filter = filter.trim();
    let (attribute, rest) = filter.split_once(char::is_whitespace).ok_or_else(invalid)?;
    let (operator, value) = rest
        .trim_start()
        .split_once(char::is_whitespace)
        .ok_or_else(invalid)?;
    if !operator.eq_ignore_ascii_case("eq") {
        return Err(invalid());
    }
    let value: String = serde_json::from_str(value.trim()).map_err(|_json| invalid())?;
    Ok(Filter {
        attribute: attribute.to_ascii_lowercase(),
        value,
    })
}

/// A PATCH request (RFC 7644 §3.5.2).
#[derive(Debug, Deserialize)]
pub struct PatchRequest {
    /// Message schemas; must include the PATCH one.
    #[serde(default)]
    pub schemas: Vec<String>,
    /// The operations, applied in order.
    #[serde(rename = "Operations")]
    pub operations: Vec<PatchOperation>,
}

/// One PATCH operation.
#[derive(Debug, Deserialize)]
pub struct PatchOperation {
    /// `add`, `remove` or `replace` (any case).
    pub op: String,
    /// Target attribute.
    #[serde(default)]
    pub path: Option<String>,
    /// New value.
    #[serde(default)]
    pub value: Option<Value>,
}

impl PatchRequest {
    /// Checks the message schema.
    ///
    /// # Errors
    ///
    /// `invalidSyntax` if the PATCH message schema is missing.
    pub fn check(&self) -> Result<(), ScimError> {
        if self.schemas.iter().any(|s| s == PATCH_SCHEMA) {
            Ok(())
        } else {
            Err(ScimError::bad(
                "invalidSyntax",
                "missing the PatchOp schema",
            ))
        }
    }
}

/// The operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `add`.
    Add,
    /// `remove`.
    Remove,
    /// `replace`.
    Replace,
}

impl PatchOperation {
    /// The kind of operation.
    ///
    /// # Errors
    ///
    /// `invalidSyntax` for an unknown `op`.
    pub fn kind(&self) -> Result<Op, ScimError> {
        match self.op.to_ascii_lowercase().as_str() {
            "add" => Ok(Op::Add),
            "remove" => Ok(Op::Remove),
            "replace" => Ok(Op::Replace),
            _ => Err(ScimError::bad(
                "invalidSyntax",
                "op must be add, remove or replace",
            )),
        }
    }
}

/// A boolean, also accepting the strings `"true"` and `"false"` some identity providers send.
fn boolean(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| {
        let text = value.as_str()?;
        if text.eq_ignore_ascii_case("true") {
            Some(true)
        } else if text.eq_ignore_ascii_case("false") {
            Some(false)
        } else {
            None
        }
    })
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Sets one user attribute from a request. Attributes Genkan does not store (emails, names,
/// titles…) are ignored, so that identity providers that send them keep working.
fn set_user_attribute(
    user: &mut ManagedUser,
    attribute: &str,
    value: Option<&Value>,
) -> Result<(), ScimError> {
    let invalid = |what: &str| ScimError::bad("invalidValue", format!("{what} has the wrong type"));
    match attribute.to_ascii_lowercase().as_str() {
        "active" => {
            let active = value.and_then(boolean).ok_or_else(|| invalid("active"))?;
            user.disabled = !active;
        }
        "displayname" => user.display_name = value.and_then(text),
        "externalid" => user.external_id = value.and_then(text),
        "username" => {
            let requested = value.and_then(text).ok_or_else(|| invalid("userName"))?;
            if requested.to_ascii_lowercase() != user.username {
                return Err(ScimError::bad("mutability", "userName cannot change"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Applies user fields from a full resource (POST, PUT).
///
/// # Errors
///
/// `invalidValue` for a wrongly typed attribute, `mutability` for a changed `userName`.
pub fn apply_user_resource(user: &mut ManagedUser, body: &Value) -> Result<(), ScimError> {
    let object = body
        .as_object()
        .ok_or_else(|| ScimError::bad("invalidSyntax", "expected a JSON object"))?;
    let display = object
        .get("displayName")
        .cloned()
        .or_else(|| object.get("name").and_then(|n| n.get("formatted")).cloned());
    user.display_name = display.as_ref().and_then(text);
    user.external_id = object.get("externalId").and_then(text);
    if let Some(active) = object.get("active") {
        user.disabled = !boolean(active).ok_or_else(|| ScimError::bad("invalidValue", "active"))?;
    } else {
        user.disabled = false;
    }
    Ok(())
}

/// Applies PATCH operations to a user.
///
/// # Errors
///
/// `invalidSyntax`, `invalidValue` or `mutability` as RFC 7644 §3.5.2 describes.
pub fn apply_user_patch(user: &mut ManagedUser, request: &PatchRequest) -> Result<(), ScimError> {
    request.check()?;
    for operation in &request.operations {
        let op = operation.kind()?;
        match (op, &operation.path) {
            (Op::Remove, Some(path)) => set_user_attribute(user, path, None)?,
            (Op::Remove, None) => {
                return Err(ScimError::bad("noTarget", "remove needs a path"));
            }
            (Op::Add | Op::Replace, Some(path)) => {
                set_user_attribute(user, path, operation.value.as_ref())?;
            }
            (Op::Add | Op::Replace, None) => {
                let object = operation
                    .value
                    .as_ref()
                    .and_then(Value::as_object)
                    .ok_or_else(|| ScimError::bad("invalidValue", "expected an object"))?;
                for (attribute, value) in object {
                    set_user_attribute(user, attribute, Some(value))?;
                }
            }
        }
    }
    Ok(())
}

/// The member IDs in a `members` value: `[{"value": "<id>"}, …]`.
fn member_ids(value: Option<&Value>) -> Result<Vec<String>, ScimError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list = value
        .get("members")
        .unwrap_or(value)
        .as_array()
        .ok_or_else(|| ScimError::bad("invalidValue", "members must be a list"))?;
    list.iter()
        .map(|m| {
            m.get("value")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| ScimError::bad("invalidValue", "a member needs a value"))
        })
        .collect()
}

/// The full member list of a group resource (POST, PUT).
///
/// # Errors
///
/// `invalidValue` if `members` is malformed.
pub fn group_members(body: &Value) -> Result<Vec<String>, ScimError> {
    member_ids(body.get("members"))
}

/// The new member set after applying PATCH operations to `members`.
///
/// # Errors
///
/// `invalidSyntax`, `invalidPath` or `invalidValue`.
pub fn apply_group_patch(
    mut members: Vec<String>,
    request: &PatchRequest,
) -> Result<Vec<String>, ScimError> {
    request.check()?;
    for operation in &request.operations {
        let op = operation.kind()?;
        let path = operation.path.as_deref().map(str::trim);
        // `members[value eq "<id>"]`: one member by filter.
        let filtered = path
            .and_then(|p| {
                p.strip_prefix("members[")
                    .or_else(|| p.strip_prefix("Members["))
            })
            .and_then(|p| p.strip_suffix(']'))
            .map(parse_filter)
            .transpose()?;
        let targets_members = path.is_none_or(|p| p.eq_ignore_ascii_case("members"));
        match (op, filtered) {
            (Op::Remove, Some(filter)) if filter.attribute == "value" => {
                members.retain(|m| *m != filter.value);
            }
            (_, Some(_)) => return Err(ScimError::bad("invalidPath", "unsupported filter")),
            (_, None) if !targets_members => {
                // `displayName` changes are not supported: the name is the group.
                if path.is_some_and(|p| p.eq_ignore_ascii_case("displayName")) {
                    return Err(ScimError::bad("mutability", "a group cannot be renamed"));
                }
            }
            (Op::Add, None) => {
                for id in member_ids(operation.value.as_ref())? {
                    if !members.contains(&id) {
                        members.push(id);
                    }
                }
            }
            (Op::Remove, None) => {
                if operation.value.is_none() {
                    members.clear();
                } else {
                    let gone = member_ids(operation.value.as_ref())?;
                    members.retain(|m| !gone.contains(m));
                }
            }
            (Op::Replace, None) => members = member_ids(operation.value.as_ref())?,
        }
    }
    Ok(members)
}

/// `GET /ServiceProviderConfig` (RFC 7643 §5).
#[must_use]
pub fn service_provider_config(base: &str) -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
        "patch": { "supported": true },
        "bulk": { "supported": false, "maxOperations": 0, "maxPayloadSize": 0 },
        "filter": { "supported": true, "maxResults": 200 },
        "changePassword": { "supported": false },
        "sort": { "supported": false },
        "etag": { "supported": false },
        "authenticationSchemes": [{
            "type": "oauthbearertoken",
            "name": "Bearer token",
            "description": "A static bearer token configured in Genkan",
            "primary": true,
        }],
        "meta": {
            "resourceType": "ServiceProviderConfig",
            "location": format!("{base}/ServiceProviderConfig"),
        },
    })
}

/// `GET /ResourceTypes` (RFC 7643 §6).
#[must_use]
pub fn resource_types(base: &str) -> Value {
    let resource = |id: &str, endpoint: &str, schema: &str| {
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
            "id": id,
            "name": id,
            "endpoint": endpoint,
            "schema": schema,
            "meta": { "resourceType": "ResourceType", "location": format!("{base}/ResourceTypes/{id}") },
        })
    };
    list_json(
        vec![
            resource("User", "/Users", USER_SCHEMA),
            resource("Group", "/Groups", GROUP_SCHEMA),
        ],
        1,
        2,
    )
}

/// `GET /Schemas` (RFC 7643 §7): the attributes Genkan stores.
#[must_use]
pub fn schemas() -> Value {
    let attribute = |name: &str, kind: &str, mutability: &str, required: bool| {
        json!({
            "name": name, "type": kind, "multiValued": false, "required": required,
            "caseExact": false, "mutability": mutability, "returned": "default", "uniqueness": if name == "userName" { "server" } else { "none" },
        })
    };
    list_json(
        vec![
            json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
                "id": USER_SCHEMA,
                "name": "User",
                "attributes": [
                    attribute("userName", "string", "immutable", true),
                    attribute("displayName", "string", "readWrite", false),
                    attribute("active", "boolean", "readWrite", false),
                    attribute("externalId", "string", "readWrite", false),
                ],
            }),
            json!({
                "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
                "id": GROUP_SCHEMA,
                "name": "Group",
                "attributes": [attribute("displayName", "string", "immutable", true)],
            }),
        ],
        1,
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(ops: &Value) -> PatchRequest {
        serde_json::from_value(json!({ "schemas": [PATCH_SCHEMA], "Operations": ops })).unwrap()
    }

    #[test]
    fn filters() {
        assert_eq!(
            parse_filter(r#"userName eq "Alice@Example.com""#).unwrap(),
            Filter {
                attribute: "username".into(),
                value: "Alice@Example.com".into()
            }
        );
        for bad in ["userName", r#"userName co "a""#, "userName eq alice", ""] {
            assert!(parse_filter(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn user_patches_follow_what_identity_providers_send() {
        let mut user = ManagedUser {
            username: "alice@example.com".into(),
            ..ManagedUser::default()
        };
        // Capitalised op and a string boolean.
        apply_user_patch(
            &mut user,
            &patch(&json!([{ "op": "Replace", "path": "active", "value": "False" }])),
        )
        .unwrap();
        assert!(user.disabled);
        // No path: an object of attributes, unknown ones ignored.
        apply_user_patch(
            &mut user,
            &patch(&json!([{ "op": "replace", "value": { "active": true, "displayName": "Alice", "title": "x" } }])),
        )
        .unwrap();
        assert!(!user.disabled);
        assert_eq!(user.display_name.as_deref(), Some("Alice"));
        apply_user_patch(
            &mut user,
            &patch(&json!([{ "op": "remove", "path": "displayName" }])),
        )
        .unwrap();
        assert_eq!(user.display_name, None);
        // userName cannot change (case-insensitively equal is fine).
        assert!(
            apply_user_patch(
                &mut user,
                &patch(
                    &json!([{ "op": "replace", "path": "userName", "value": "ALICE@example.com" }])
                )
            )
            .is_ok()
        );
        assert_eq!(
            apply_user_patch(
                &mut user,
                &patch(&json!([{ "op": "replace", "path": "userName", "value": "bob" }]))
            )
            .unwrap_err()
            .scim_type,
            Some("mutability")
        );
        // Without the PatchOp schema.
        let request: PatchRequest = serde_json::from_value(json!({ "Operations": [] })).unwrap();
        assert!(apply_user_patch(&mut user, &request).is_err());
    }

    #[test]
    fn group_patches() {
        let members = vec!["u-1".to_owned(), "u-2".to_owned()];
        let added = apply_group_patch(
            members.clone(),
            &patch(&json!([{ "op": "add", "path": "members", "value": [{ "value": "u-3" }, { "value": "u-1" }] }])),
        )
        .unwrap();
        assert_eq!(added, ["u-1", "u-2", "u-3"]);
        let removed = apply_group_patch(
            added,
            &patch(&json!([{ "op": "remove", "path": "members[value eq \"u-2\"]" }])),
        )
        .unwrap();
        assert_eq!(removed, ["u-1", "u-3"]);
        let replaced = apply_group_patch(
            removed,
            &patch(&json!([{ "op": "replace", "value": { "members": [{ "value": "u-9" }] } }])),
        )
        .unwrap();
        assert_eq!(replaced, ["u-9"]);
        assert!(
            apply_group_patch(
                members,
                &patch(&json!([{ "op": "replace", "path": "displayName", "value": "x" }]))
            )
            .is_err()
        );
    }

    #[test]
    fn lists_paginate() {
        let items: Vec<Value> = (0..5).map(|i| json!(i)).collect();
        let page = list_json(items, 2, 2);
        assert_eq!(page["totalResults"], 5);
        assert_eq!(page["itemsPerPage"], 2);
        assert_eq!(page["Resources"], json!([1, 2]));
    }
}
