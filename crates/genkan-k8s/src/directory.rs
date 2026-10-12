//! [`Directory`] backed by `User` and `OAuthClient` objects.
//!
//! Every lookup is a direct GET, so a disabled user or a changed client takes effect on the next
//! request. At the expected scale (tens of users) this costs less memory than an informer cache.

use std::future::Future;

use genkan_keys::Algorithm;
use genkan_store::{Client, Directory, StoreError, User, unique_user};
use kube::ResourceExt as _;
use kube::api::{Api, ListParams};

use crate::crd::client::OAuthClient;
use crate::crd::user::User as UserObject;

/// Reads clients and users from the API server.
#[derive(Clone)]
pub struct KubeDirectory {
    clients: Api<OAuthClient>,
    users: Api<UserObject>,
}

impl std::fmt::Debug for KubeDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KubeDirectory")
    }
}

impl KubeDirectory {
    /// Uses the given (namespaced) APIs.
    #[must_use]
    pub const fn new(clients: Api<OAuthClient>, users: Api<UserObject>) -> Self {
        Self { clients, users }
    }
}

fn backend(error: &kube::Error) -> StoreError {
    StoreError(error.to_string())
}

impl Directory for KubeDirectory {
    fn client(&self, id: &str) -> impl Future<Output = Result<Option<Client>, StoreError>> + Send {
        let api = self.clients.clone();
        let id = id.to_owned();
        async move {
            let object = api.get_opt(&id).await.map_err(|e| backend(&e))?;
            Ok(object.map(|object| {
                let id = object.name_any();
                let spec = object.spec;
                Client {
                    id,
                    display_name: spec.display_name,
                    kind: spec.kind,
                    redirect_uris: spec.redirect_uris,
                    audiences: spec.audiences,
                    secret_hash: spec.secret_hash,
                    id_token_alg: spec.id_token_signing_alg.unwrap_or(Algorithm::Es256),
                }
            }))
        }
    }

    fn user(&self, name: &str) -> impl Future<Output = Result<Option<User>, StoreError>> + Send {
        let api = self.users.clone();
        let name = name.to_owned();
        async move {
            let object = api.get_opt(&name).await.map_err(|e| backend(&e))?;
            Ok(object.and_then(convert))
        }
    }

    fn user_by_subject(
        &self,
        subject: &str,
    ) -> impl Future<Output = Result<Option<User>, StoreError>> + Send {
        let api = self.users.clone();
        let subject = subject.to_owned();
        async move {
            let users = list(&api).await?;
            unique_user(&users, |u| u.subject == subject)
        }
    }
}

/// Lists every user. At the expected scale (tens of users) a list per lookup is cheaper than a
/// cache; lookups by subject are rare (UserInfo).
async fn list(api: &Api<UserObject>) -> Result<Vec<User>, StoreError> {
    let objects = api
        .list(&ListParams::default())
        .await
        .map_err(|e| backend(&e))?;
    Ok(objects.items.into_iter().filter_map(convert).collect())
}

/// An object without a UID was never stored; it is treated as absent.
fn convert(object: UserObject) -> Option<User> {
    let subject = object.uid()?;
    Some(User {
        name: object.name_any(),
        subject,
        username: object.spec.username,
        display_name: object.spec.display_name,
        groups: object.spec.groups,
        disabled: object.spec.disabled,
        elevated_groups: object.spec.elevated_groups,
    })
}
