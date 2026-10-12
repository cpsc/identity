//! What SCIM provisioning reads and writes: only users it created.
//!
//! Writes carry the version they were based on, so a replace is one compare-and-swap and a
//! PATCH stays atomic (RFC 7644 §3.5.2). [`Write::Exists`] is a uniqueness conflict, which SCIM
//! reports as 409 (RFC 7644 §3.3).

use std::collections::HashMap;
use std::future::{self, Future};
use std::sync::{Mutex, PoisonError};

use crate::StoreError;

/// A user as SCIM sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManagedUser {
    /// Object name; the SCIM `id`.
    pub name: String,
    /// Opaque version for compare-and-swap; the SCIM `meta.version`.
    pub version: String,
    /// Login name (lowercase).
    pub username: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Groups (always within the allowlist).
    pub groups: Vec<String>,
    /// `active = false`.
    pub disabled: bool,
    /// The identity provider's own identifier.
    pub external_id: Option<String>,
    /// Creation time, RFC 3339.
    pub created: Option<String>,
}

/// Outcome of a write that carries the expected version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// Applied; the stored user.
    Done(ManagedUser),
    /// Someone changed the user meanwhile.
    Conflict,
    /// The name or username is taken.
    Exists,
    /// No such managed user.
    NotFound,
}

/// Users created through SCIM.
pub trait UserAdmin: Send + Sync + 'static {
    /// Every managed user.
    fn list(&self) -> impl Future<Output = Result<Vec<ManagedUser>, StoreError>> + Send;
    /// One managed user.
    fn get(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<ManagedUser>, StoreError>> + Send;
    /// Whether any user, managed or not, has this username.
    fn username_taken(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
    /// Creates a managed user; `version` is ignored.
    fn create(&self, user: &ManagedUser) -> impl Future<Output = Result<Write, StoreError>> + Send;
    /// Replaces a managed user if it is still at `user.version`.
    fn replace(&self, user: &ManagedUser)
    -> impl Future<Output = Result<Write, StoreError>> + Send;
    /// Deletes a managed user.
    fn delete(&self, name: &str) -> impl Future<Output = Result<Write, StoreError>> + Send;
}

/// In-memory users, for tests and `--store memory`.
#[derive(Debug, Default)]
pub struct MemoryUserAdmin {
    users: Mutex<HashMap<String, ManagedUser>>,
    /// Usernames of users SCIM did not create.
    others: Mutex<Vec<String>>,
    next: Mutex<u64>,
}

impl MemoryUserAdmin {
    /// Records a user created by other means (for example the setup link).
    pub fn put_other(&self, username: &str) {
        self.others
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(username.to_owned());
    }

    fn version(&self) -> String {
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        *next += 1;
        next.to_string()
    }
}

impl UserAdmin for MemoryUserAdmin {
    fn list(&self) -> impl Future<Output = Result<Vec<ManagedUser>, StoreError>> + Send {
        let users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let mut list: Vec<ManagedUser> = users.values().cloned().collect();
        drop(users);
        list.sort_by(|a, b| a.name.cmp(&b.name));
        future::ready(Ok(list))
    }

    fn get(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<ManagedUser>, StoreError>> + Send {
        let users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        future::ready(Ok(users.get(name).cloned()))
    }

    fn username_taken(
        &self,
        username: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        let managed = self
            .users
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|u| u.username == username);
        let other = self
            .others
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|u| u == username);
        future::ready(Ok(managed || other))
    }

    fn create(&self, user: &ManagedUser) -> impl Future<Output = Result<Write, StoreError>> + Send {
        let version = self.version();
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = if users.contains_key(&user.name) {
            Write::Exists
        } else {
            let stored = ManagedUser {
                version,
                created: Some("2026-01-01T00:00:00Z".to_owned()),
                ..user.clone()
            };
            users.insert(user.name.clone(), stored.clone());
            Write::Done(stored)
        };
        drop(users);
        future::ready(Ok(outcome))
    }

    fn replace(
        &self,
        user: &ManagedUser,
    ) -> impl Future<Output = Result<Write, StoreError>> + Send {
        let version = self.version();
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = match users.get(&user.name) {
            None => Write::NotFound,
            Some(current) if current.version != user.version => Write::Conflict,
            Some(current) => {
                let stored = ManagedUser {
                    version,
                    created: current.created.clone(),
                    ..user.clone()
                };
                users.insert(user.name.clone(), stored.clone());
                Write::Done(stored)
            }
        };
        drop(users);
        future::ready(Ok(outcome))
    }

    fn delete(&self, name: &str) -> impl Future<Output = Result<Write, StoreError>> + Send {
        let mut users = self.users.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = users.remove(name).map_or(Write::NotFound, Write::Done);
        drop(users);
        future::ready(Ok(outcome))
    }
}
