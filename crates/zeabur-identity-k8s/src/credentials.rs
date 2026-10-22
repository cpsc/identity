//! [`CredentialStore`] backed by `Credential` objects.
//!
//! Renaming and last-use times go to `status`, so the spec (the key material) stays immutable.
//! Removing a passkey must never leave a user with none, also when two removals race: see
//! [`KubeCredentialStore::remove`].

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{Event, EventSource, ObjectReference};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::jiff::Timestamp;
use kube::ResourceExt as _;
use kube::api::{Api, ListParams, ObjectMeta, Patch, PatchParams, PostParams};
use zeabur_identity_core::clock::UnixTime;
use zeabur_identity_store::{
    AccountEvent, CreateCredential, CredentialStore, NewExternal, NewPasskey, PasskeySummary,
    Removal, Rename, StoreError, StoredPasskey, User,
};

use super::MANAGED_BY;
use super::objects::{
    CreateOutcome, DeleteOutcome, ReplaceOutcome, create_once, delete_if_unchanged,
    replace_status_if_unchanged,
};
use super::users::{owned_by_another, user_owner};
use crate::crd::credential::{
    Credential, CredentialSpec, CredentialType, ExternalIdentity, Passkey,
};
use crate::crd::{GROUP, USER_LABEL};

/// How many times a rename re-reads after losing a compare-and-swap.
const MAX_ATTEMPTS: usize = 4;

/// Passkeys as `Credential` objects named by their credential ID.
#[derive(Clone)]
pub struct KubeCredentialStore {
    api: Api<Credential>,
    events: Option<Api<Event>>,
}

impl std::fmt::Debug for KubeCredentialStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KubeCredentialStore")
    }
}

impl KubeCredentialStore {
    /// Uses the given (namespaced) API. Account events go to core `Event`s in the same
    /// namespace.
    #[must_use]
    pub fn new(api: Api<Credential>) -> Self {
        let events = api
            .namespace()
            .map(|namespace| Api::namespaced(api.clone().into_client(), namespace));
        Self { api, events }
    }
}

fn backend(error: &kube::Error) -> StoreError {
    StoreError(error.to_string())
}

fn secs(time: UnixTime) -> i64 {
    i64::try_from(time.as_secs()).unwrap_or(i64::MAX)
}

fn time(secs: i64) -> UnixTime {
    UnixTime::from_secs(u64::try_from(secs).unwrap_or(0))
}

/// The user's passkeys, read from the API server: by label, then by `spec.user`, then without
/// those owned by another `User` UID.
async fn owned_passkeys(api: &Api<Credential>, user: &User) -> Result<Vec<Credential>, StoreError> {
    let params = ListParams::default().labels(&format!("{USER_LABEL}={}", user.name));
    let list = api.list(&params).await.map_err(|e| backend(&e))?;
    Ok(list
        .items
        .into_iter()
        .filter(|object| {
            object.spec.user == user.name
                && object.spec.kind == CredentialType::Passkey
                && object.spec.passkey.is_some()
                && !owned_by_another(&object.metadata, &user.subject)
        })
        .collect())
}

fn summary(object: &Credential) -> Option<PasskeySummary> {
    let passkey = object.spec.passkey.as_ref()?;
    let status = object.status.as_ref();
    Some(PasskeySummary {
        name: object.name_any(),
        credential_id: passkey.credential_id.clone(),
        record: passkey.record.clone(),
        label: status
            .and_then(|s| s.label.clone())
            .or_else(|| passkey.label.clone()),
        aaguid: passkey.aaguid.clone(),
        created_at: object
            .metadata
            .creation_timestamp
            .as_ref()
            .map(|t| time(t.0.as_second())),
        last_used_at: status.and_then(|s| s.last_used_at).map(time),
    })
}

impl CredentialStore for KubeCredentialStore {
    fn create(
        &self,
        passkey: &NewPasskey,
    ) -> impl Future<Output = Result<CreateCredential, StoreError>> + Send {
        let api = self.api.clone();
        let mut object = Credential {
            metadata: ObjectMeta {
                name: Some(passkey.name.clone()),
                labels: Some(BTreeMap::from([
                    (USER_LABEL.to_owned(), passkey.user.clone()),
                    (MANAGED_BY.0.to_owned(), MANAGED_BY.1.to_owned()),
                ])),
                ..ObjectMeta::default()
            },
            spec: CredentialSpec {
                user: passkey.user.clone(),
                kind: CredentialType::Passkey,
                passkey: Some(Passkey {
                    credential_id: passkey.credential_id.clone(),
                    record: passkey.record.clone(),
                    backup_eligible: passkey.backup_eligible,
                    aaguid: passkey.aaguid.clone(),
                    label: passkey.label.clone(),
                }),
                external: None,
            },
            status: None,
        };
        async move {
            object.metadata.owner_references = user_owner(&api, &object.spec.user)
                .await?
                .map(|owner| vec![owner]);
            match create_once(&api, &object).await.map_err(|e| backend(&e))? {
                CreateOutcome::Created(_) => Ok(CreateCredential::Created),
                CreateOutcome::AlreadyExists => Ok(CreateCredential::AlreadyExists),
            }
        }
    }

    fn get(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<StoredPasskey>, StoreError>> + Send {
        let api = self.api.clone();
        let name = name.to_owned();
        async move {
            let object = api.get_opt(&name).await.map_err(|e| backend(&e))?;
            Ok(object.and_then(|object| {
                let last_used_at = object
                    .status
                    .as_ref()
                    .and_then(|s| s.last_used_at)
                    .map(time);
                let spec = object.spec;
                spec.passkey
                    .filter(|_| spec.kind == CredentialType::Passkey)
                    .map(|passkey| StoredPasskey {
                        user: spec.user,
                        record: passkey.record,
                        last_used_at,
                    })
            }))
        }
    }

    fn link(
        &self,
        external: &NewExternal,
    ) -> impl Future<Output = Result<CreateCredential, StoreError>> + Send {
        let api = self.api.clone();
        let mut object = Credential {
            metadata: ObjectMeta {
                name: Some(external.name.clone()),
                labels: Some(BTreeMap::from([
                    (USER_LABEL.to_owned(), external.user.clone()),
                    (MANAGED_BY.0.to_owned(), MANAGED_BY.1.to_owned()),
                ])),
                ..ObjectMeta::default()
            },
            spec: CredentialSpec {
                user: external.user.clone(),
                kind: external.kind,
                passkey: None,
                external: Some(ExternalIdentity {
                    issuer: external.issuer.clone(),
                    subject: external.subject.clone(),
                }),
            },
            status: None,
        };
        async move {
            object.metadata.owner_references = user_owner(&api, &object.spec.user)
                .await?
                .map(|owner| vec![owner]);
            match create_once(&api, &object).await.map_err(|e| backend(&e))? {
                CreateOutcome::Created(_) => Ok(CreateCredential::Created),
                CreateOutcome::AlreadyExists => Ok(CreateCredential::AlreadyExists),
            }
        }
    }

    fn linked_user(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        let api = self.api.clone();
        let name = name.to_owned();
        async move {
            let object = api.get_opt(&name).await.map_err(|e| backend(&e))?;
            Ok(object
                .filter(|o| o.spec.kind != CredentialType::Passkey && o.spec.external.is_some())
                .map(|o| o.spec.user))
        }
    }

    fn passkeys(
        &self,
        user: &User,
    ) -> impl Future<Output = Result<Vec<PasskeySummary>, StoreError>> + Send {
        let api = self.api.clone();
        let user = user.clone();
        async move {
            let mut list: Vec<PasskeySummary> = owned_passkeys(&api, &user)
                .await?
                .iter()
                .filter_map(summary)
                .collect();
            list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.name.cmp(&b.name)));
            Ok(list)
        }
    }

    fn rename(
        &self,
        user: &User,
        name: &str,
        label: &str,
    ) -> impl Future<Output = Result<Rename, StoreError>> + Send {
        let api = self.api.clone();
        let (user, name, label) = (user.clone(), name.to_owned(), label.to_owned());
        async move {
            for _ in 0..MAX_ATTEMPTS {
                let Some(mut object) = owned_passkeys(&api, &user)
                    .await?
                    .into_iter()
                    .find(|object| object.name_any() == name)
                else {
                    return Ok(Rename::NotFound);
                };
                let mut status = object.status.take().unwrap_or_default();
                status.label = Some(label.clone());
                status.revision = status.revision.saturating_add(1);
                object.status = Some(status);
                match replace_status_if_unchanged(&api, &object)
                    .await
                    .map_err(|e| backend(&e))?
                {
                    ReplaceOutcome::Replaced(_) => return Ok(Rename::Renamed),
                    ReplaceOutcome::NotFound => return Ok(Rename::NotFound),
                    ReplaceOutcome::Conflict => {}
                }
            }
            Err(StoreError(
                "renaming the passkey kept conflicting".to_owned(),
            ))
        }
    }

    /// Deletes `name` if the user keeps at least one other passkey, in three steps against the
    /// API server: list the user's passkeys; bump `status.revision` on every other one with a
    /// compare-and-swap on the listed `resourceVersion`; delete `name` with preconditions on its
    /// listed UID and `resourceVersion`. Any lost race aborts with [`Removal::Conflict`].
    ///
    /// Two removals cannot both succeed and leave nothing: each must write every passkey it
    /// keeps and then delete one it listed unchanged. If removal A deletes `a` while B deletes
    /// `b`, A's write to `b` either comes before B lists (B then sees `a` gone and refuses as the
    /// last passkey, or sees `b` changed and its delete fails) or after (B's earlier write to
    /// `a` makes A's delete fail). Creating a passkey concurrently only adds one.
    fn remove(
        &self,
        user: &User,
        name: &str,
    ) -> impl Future<Output = Result<Removal, StoreError>> + Send {
        let api = self.api.clone();
        let (user, name) = (user.clone(), name.to_owned());
        async move {
            let passkeys = owned_passkeys(&api, &user).await?;
            let Some(target) = passkeys.iter().find(|object| object.name_any() == name) else {
                return Ok(Removal::NotFound);
            };
            if passkeys.len() < 2 {
                return Ok(Removal::LastPasskey);
            }
            let (Some(uid), Some(version)) = (
                target.metadata.uid.clone(),
                target.metadata.resource_version.clone(),
            ) else {
                return Err(StoreError(
                    "the passkey has no uid or resourceVersion".to_owned(),
                ));
            };
            for other in passkeys.iter().filter(|object| object.name_any() != name) {
                let mut other = other.clone();
                let mut status = other.status.take().unwrap_or_default();
                status.revision = status.revision.saturating_add(1);
                other.status = Some(status);
                match replace_status_if_unchanged(&api, &other)
                    .await
                    .map_err(|e| backend(&e))?
                {
                    ReplaceOutcome::Replaced(_) => {}
                    ReplaceOutcome::Conflict | ReplaceOutcome::NotFound => {
                        return Ok(Removal::Conflict);
                    }
                }
            }
            match delete_if_unchanged(&api, &name, &uid, &version)
                .await
                .map_err(|e| backend(&e))?
            {
                DeleteOutcome::Deleted => Ok(Removal::Removed),
                DeleteOutcome::Changed | DeleteOutcome::NotFound => Ok(Removal::Conflict),
            }
        }
    }

    fn touch(
        &self,
        name: &str,
        now: UnixTime,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        let api = self.api.clone();
        let name = name.to_owned();
        // A merge patch without a resourceVersion: it never conflicts with, or undoes, a rename,
        // and losing one to a race costs only a slightly older time.
        let patch = serde_json::json!({ "status": { "lastUsedAt": secs(now) } });
        async move {
            match api
                .patch_status(&name, &PatchParams::default(), &Patch::Merge(patch))
                .await
            {
                Ok(_) => Ok(()),
                Err(kube::Error::Api(status)) if status.code == 404 => Ok(()),
                Err(error) => Err(backend(&error)),
            }
        }
    }

    fn record(&self, event: &AccountEvent) -> impl Future<Output = Result<(), StoreError>> + Send {
        let events = self.events.clone();
        let namespace = self.api.namespace().map(str::to_owned);
        let at = Timestamp::from_second(secs(event.at)).ok().map(Time);
        let object = Event {
            metadata: ObjectMeta {
                generate_name: Some(format!("{}.", event.user)),
                labels: Some(BTreeMap::from([
                    (USER_LABEL.to_owned(), event.user.clone()),
                    (MANAGED_BY.0.to_owned(), MANAGED_BY.1.to_owned()),
                ])),
                ..ObjectMeta::default()
            },
            involved_object: ObjectReference {
                api_version: Some(format!("{GROUP}/v1alpha1")),
                kind: Some("User".to_owned()),
                name: Some(event.user.clone()),
                namespace,
                uid: Some(event.subject.clone()),
                ..ObjectReference::default()
            },
            reason: Some(event.reason.to_owned()),
            message: Some(event.note.clone()),
            type_: Some("Normal".to_owned()),
            count: Some(1),
            first_timestamp: at.clone(),
            last_timestamp: at,
            source: Some(EventSource {
                component: Some(MANAGED_BY.1.to_owned()),
                host: None,
            }),
            reporting_component: Some(MANAGED_BY.1.to_owned()),
            ..Event::default()
        };
        async move {
            let events =
                events.ok_or_else(|| StoreError("events need a namespaced API".to_owned()))?;
            events
                .create(&PostParams::default(), &object)
                .await
                .map(|_| ())
                .map_err(|e| backend(&e))
        }
    }
}
