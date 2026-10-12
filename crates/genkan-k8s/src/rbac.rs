//! The namespace-scoped objects Genkan needs: its namespace, service account, least-privilege
//! Role, and the empty signing-key Secret.
//!
//! Generated from code so the deployment manifest and the integration tests use the same rules;
//! the tests act as the service account through impersonation and show that it cannot list
//! Secrets, read or create other Secrets, bind `cluster-admin`, create `User`s, or read grants in
//! another namespace.
//!
//! - **Secrets by name only.** `get`/`update` on the signing keys, `get`/`delete` on the setup
//!   token (redeeming is deleting), `get` on the internal certificate. RBAC cannot limit `create`
//!   or `deletecollection` to a name, and `list` or `watch` on Secrets reveals their contents
//!   ([RBAC], [RBAC good practices]), so Genkan has none of these on Secrets: the installer creates
//!   the empty key Secret and the cluster admin's `genkanctl` writes the other two.
//! - **Configuration is read-only.** Users, credentials and clients are written by admins or
//!   GitOps; Genkan reads them, writes `users/status`, and creates or deletes credentials when a
//!   passkey is enrolled or an account is linked.
//! - **Runtime state is Genkan's.** Full access to grants; invitations it can read, mail (`status`)
//!   and redeem (`delete`), but not create.
//! - **Nothing else.** No cluster-scoped rights, nothing in other namespaces, no RBAC objects, no
//!   `impersonate`. The admin bindings ([`ADMINS_BINDING`], [`ELEVATED_ADMINS_BINDING`]) are
//!   applied at install with the cluster admin's rights; Genkan cannot change them. SCIM adds
//!   writes on `users`, bounded by an admission policy ([`scim_policy`]).
//!
//! # References
//!
//! - [RBAC] The Kubernetes Authors, "Using RBAC Authorization: Referring to resources," Kubernetes
//!   documentation.
//! - [RBAC good practices] The Kubernetes Authors, "Role Based Access Control Good Practices,"
//!   Kubernetes documentation.
//!
//! [RBAC]: https://kubernetes.io/docs/reference/access-authn-authz/rbac/
//! [RBAC good practices]: https://kubernetes.io/docs/concepts/security/rbac-good-practices/

use std::collections::BTreeMap;

use genkan_core::protocol::ELEVATED_GROUP_PREFIX;
use k8s_openapi::api::core::v1::{Namespace, Secret, ServiceAccount};
use k8s_openapi::api::rbac::v1::{
    ClusterRoleBinding, PolicyRule, Role, RoleBinding, RoleRef, Subject,
};
use kube::api::ObjectMeta;
use serde_json::Value;

use super::keys::SECRET_TYPE;
use super::setup::{ADMIN_GROUP, SETUP_SECRET};
use super::{KEY_SECRET, MANAGED_BY, internal_tls};
use crate::crd::GROUP;

/// Name of the service account, `Role` and `RoleBinding`.
pub const NAME: &str = "genkan";

/// Name of the `ClusterRoleBinding` that lets `genkan:admins` read the cluster.
pub const ADMINS_BINDING: &str = "genkan-admins";

/// Name of the `ClusterRoleBinding` that makes `genkan:elevated:admins` cluster admins.
pub const ELEVATED_ADMINS_BINDING: &str = "genkan-elevated-admins";

pub use genkan_core::protocol::IDENTITY_PREFIX;

/// Default namespace.
pub const DEFAULT_NAMESPACE: &str = "genkan-system";

fn rule(groups: &[&str], resources: &[&str], names: &[&str], verbs: &[&str]) -> PolicyRule {
    let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    PolicyRule {
        api_groups: Some(strings(groups)),
        resources: Some(strings(resources)),
        resource_names: (!names.is_empty()).then(|| strings(names)),
        verbs: strings(verbs),
        ..PolicyRule::default()
    }
}

/// The rules of Genkan's Role. There is no `list`, `watch` or `create` on Secrets, and nothing
/// outside the namespace.
#[must_use]
pub fn rules() -> Vec<PolicyRule> {
    vec![
        // Signing keys: read and compare-and-swap one Secret.
        rule(&[""], &["secrets"], &[KEY_SECRET], &["get", "update"]),
        // Setup token: read and consume (delete) one Secret; only the cluster admin creates it.
        rule(&[""], &["secrets"], &[SETUP_SECRET], &["get", "delete"]),
        // Internal listener certificate: read one Secret; the cluster admin writes it.
        rule(&[""], &["secrets"], &[internal_tls::SECRET], &["get"]),
        // Configuration and identities, written by admins or GitOps.
        rule(
            &[GROUP],
            &["users", "credentials", "oauthclients"],
            &[],
            &["get", "list", "watch"],
        ),
        // Runtime state Genkan owns.
        rule(&[GROUP], &["users/status"], &[], &["update"]),
        rule(&[GROUP], &["credentials"], &[], &["create", "delete"]),
        rule(
            &[GROUP],
            &["grants"],
            &[],
            &[
                "get",
                "list",
                "watch",
                "create",
                "update",
                "delete",
                "deletecollection",
            ],
        ),
        rule(&[GROUP], &["grants/status"], &[], &["update"]),
        // Invitations: the admin creates them; Genkan redeems (deletes), mails links (status),
        // and collects expired ones.
        rule(
            &[GROUP],
            &["invitations"],
            &[],
            &["get", "list", "watch", "delete"],
        ),
        rule(&[GROUP], &["invitations/status"], &[], &["update"]),
    ]
}

/// Binds the `ClusterRole` `role` to the Kubernetes group `genkan:<group>`.
fn group_binding(name: &str, role: &str, group: &str) -> ClusterRoleBinding {
    ClusterRoleBinding {
        metadata: meta(name, None),
        role_ref: RoleRef {
            api_group: "rbac.authorization.k8s.io".to_owned(),
            kind: "ClusterRole".to_owned(),
            name: role.to_owned(),
        },
        subjects: Some(vec![Subject {
            api_group: Some("rbac.authorization.k8s.io".to_owned()),
            kind: "Group".to_owned(),
            name: format!("{IDENTITY_PREFIX}{group}"),
            ..Subject::default()
        }]),
    }
}

fn meta(name: &str, namespace: Option<&str>) -> ObjectMeta {
    ObjectMeta {
        name: Some(name.to_owned()),
        namespace: namespace.map(str::to_owned),
        labels: Some(BTreeMap::from([(
            MANAGED_BY.0.to_owned(),
            MANAGED_BY.1.to_owned(),
        )])),
        ..ObjectMeta::default()
    }
}

/// Rules added to Genkan's Role when SCIM provisioning is enabled: writing `User` objects,
/// bounded by [`scim_policy`].
#[must_use]
pub fn scim_rules() -> Vec<PolicyRule> {
    vec![rule(
        &[GROUP],
        &["users"],
        &[],
        &["create", "update", "delete"],
    )]
}

/// The admission policy and binding that bound SCIM's writes, whatever Genkan's code does.
///
/// Genkan's service account may only create, change or delete users labelled
/// `genkan.engawahq.dev/source=scim`, with groups from `groups`, and never with elevated groups.
/// RBAC cannot scope a rule by label, so the boundary is an admission policy:
/// `ValidatingAdmissionPolicy` (`admissionregistration.k8s.io/v1`, GA in Kubernetes 1.30), which
/// matches only requests from Genkan's service account and fails closed (`failurePolicy: Fail`).
/// A bug or a compromise in Genkan's SCIM code therefore cannot touch the bootstrap admin or
/// grant elevated groups.
#[must_use]
pub fn scim_policy(namespace: &str, groups: &[String]) -> Vec<Value> {
    let name = format!("genkan-scim-{namespace}");
    let source = super::users::SOURCE_LABEL;
    let scim = super::users::SCIM_SOURCE;
    let labelled = |object: &str| {
        format!(
            "has({object}.metadata.labels) && '{source}' in {object}.metadata.labels && {object}.metadata.labels['{source}'] == '{scim}'"
        )
    };
    let allowed = groups
        .iter()
        .map(|g| format!("'{g}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let validation = |expression: String, message: &str| serde_json::json!({ "expression": expression, "message": message });
    let labels = serde_json::json!({ MANAGED_BY.0: MANAGED_BY.1 });
    vec![
        serde_json::json!({
            "apiVersion": "admissionregistration.k8s.io/v1",
            "kind": "ValidatingAdmissionPolicy",
            "metadata": { "name": name, "labels": labels },
            "spec": {
                "failurePolicy": "Fail",
                "matchConstraints": {
                    "resourceRules": [{
                        "apiGroups": [GROUP],
                        "apiVersions": ["*"],
                        "operations": ["CREATE", "UPDATE", "DELETE"],
                        "resources": ["users"],
                    }],
                },
                "matchConditions": [{
                    "name": "genkan-service-account",
                    "expression": format!("request.userInfo.username == 'system:serviceaccount:{namespace}:{NAME}'"),
                }],
                "validations": [
                    validation(
                        format!("request.operation == 'CREATE' || ({})", labelled("oldObject")),
                        "Genkan may only change users created through SCIM",
                    ),
                    validation(
                        format!("request.operation == 'DELETE' || ({})", labelled("object")),
                        "users written by Genkan must keep the SCIM label",
                    ),
                    validation(
                        format!("request.operation == 'DELETE' || !has(object.spec.groups) || object.spec.groups.all(g, g in [{allowed}])"),
                        "SCIM may only assign allowlisted groups",
                    ),
                    validation(
                        "request.operation == 'DELETE' || !has(object.spec.elevatedGroups) || size(object.spec.elevatedGroups) == 0".to_owned(),
                        "SCIM never assigns elevated groups",
                    ),
                ],
            },
        }),
        serde_json::json!({
            "apiVersion": "admissionregistration.k8s.io/v1",
            "kind": "ValidatingAdmissionPolicyBinding",
            "metadata": { "name": name, "labels": labels },
            "spec": { "policyName": name, "validationActions": ["Deny"] },
        }),
    ]
}

/// The objects to install in `namespace`, in apply order.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn manifests(namespace: &str) -> serde_json::Result<Vec<Value>> {
    manifests_with(namespace, None)
}

/// [`manifests`], plus SCIM's rules and admission policy when `scim_groups` is given.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn manifests_with(
    namespace: &str,
    scim_groups: Option<&[String]>,
) -> serde_json::Result<Vec<Value>> {
    let mut role_rules = rules();
    if scim_groups.is_some() {
        role_rules.extend(scim_rules());
    }
    let ns = Some(namespace);
    let mut items = vec![
        serde_json::to_value(Namespace {
            metadata: meta(namespace, None),
            ..Namespace::default()
        })?,
        serde_json::to_value(ServiceAccount {
            metadata: meta(NAME, ns),
            automount_service_account_token: Some(true),
            ..ServiceAccount::default()
        })?,
        serde_json::to_value(Role {
            metadata: meta(NAME, ns),
            rules: Some(role_rules),
        })?,
        serde_json::to_value(RoleBinding {
            metadata: meta(NAME, ns),
            role_ref: RoleRef {
                api_group: "rbac.authorization.k8s.io".to_owned(),
                kind: "Role".to_owned(),
                name: NAME.to_owned(),
            },
            subjects: Some(vec![Subject {
                kind: "ServiceAccount".to_owned(),
                name: NAME.to_owned(),
                namespace: Some(namespace.to_owned()),
                ..Subject::default()
            }]),
        })?,
        // Least privilege by default: `genkan:admins` can read (the built-in `view` role, which
        // excludes Secrets), and becomes cluster admin only as `genkan:elevated:admins`, which
        // exists only in short tokens issued right after a passkey sign-in. Applied by the
        // installer with the cluster admin's own rights; Genkan's service account cannot create
        // bindings.
        serde_json::to_value(group_binding(ADMINS_BINDING, "view", ADMIN_GROUP))?,
        serde_json::to_value(group_binding(
            ELEVATED_ADMINS_BINDING,
            "cluster-admin",
            &format!("{ELEVATED_GROUP_PREFIX}{ADMIN_GROUP}"),
        ))?,
        // Created empty by the installer; Genkan fills it.
        serde_json::to_value(Secret {
            metadata: meta(KEY_SECRET, ns),
            type_: Some(SECRET_TYPE.to_owned()),
            ..Secret::default()
        })?,
    ];
    if let Some(groups) = scim_groups {
        items.extend(scim_policy(namespace, groups));
    }
    Ok(items)
}

/// Renders [`manifests`] as a Kubernetes `List` without a trailing newline.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn manifests_json(namespace: &str) -> serde_json::Result<String> {
    manifests_json_with(namespace, None)
}

/// Renders [`manifests_with`] as a Kubernetes `List` without a trailing newline.
///
/// # Errors
///
/// Returns an error if serialization fails.
pub fn manifests_json_with(
    namespace: &str,
    scim_groups: Option<&[String]>,
) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "apiVersion": "v1",
        "kind": "List",
        "items": manifests_with(namespace, scim_groups)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regenerate with `cargo run -q -- rbac > deploy/rbac.json`.
    #[test]
    fn committed_manifest_is_current() {
        let committed = include_str!("../../../deploy/rbac.json");
        // Compare parsed JSON: test builds may enable serde_json features (such as
        // `preserve_order`, through dev-dependencies) that change key order but not
        // content.
        let parse = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert!(
            committed.ends_with('\n')
                && parse(committed) == parse(&manifests_json(DEFAULT_NAMESPACE).unwrap()),
            "deploy/rbac.json is stale: run `cargo run -q -- rbac > deploy/rbac.json`"
        );
    }

    #[test]
    fn secrets_are_reachable_only_by_name() {
        for rule in rules() {
            if rule
                .resources
                .as_deref()
                .is_some_and(|r| r.iter().any(|r| r == "secrets"))
            {
                let names = rule.resource_names.clone().unwrap_or_default();
                assert!(
                    names == [KEY_SECRET]
                        || names == [SETUP_SECRET]
                        || names == [internal_tls::SECRET],
                    "{names:?}"
                );
                assert!(
                    rule.verbs
                        .iter()
                        .all(|v| ["get", "update", "delete"].contains(&v.as_str())),
                    "{:?}",
                    rule.verbs
                );
            }
        }
    }

    #[test]
    fn no_privilege_escalation_verbs() {
        for rule in rules() {
            for verb in &rule.verbs {
                assert!(!["*", "bind", "escalate", "impersonate"].contains(&verb.as_str()));
            }
            assert!(
                rule.api_groups
                    .iter()
                    .flatten()
                    .all(|g| g != "rbac.authorization.k8s.io")
            );
        }
    }
}
