//! Names that several parts of Genkan, and kube-apiserver's configuration, must agree on.

/// Path of the authorization endpoint.
pub const AUTHORIZATION_PATH: &str = "/authorize";
/// Path of the token endpoint.
pub const TOKEN_PATH: &str = "/token";
/// Path of the pushed authorization request endpoint (RFC 9126).
pub const PAR_PATH: &str = "/par";
/// Path of the revocation endpoint (RFC 7009).
pub const REVOCATION_PATH: &str = "/revoke";
/// Path of the JWK set.
pub const JWKS_PATH: &str = "/jwks";
/// OpenID Connect Discovery location (Discovery §4).
pub const OPENID_CONFIGURATION_PATH: &str = "/.well-known/openid-configuration";
/// UserInfo endpoint (OIDC Core §5.3).
pub const USERINFO_PATH: &str = "/userinfo";
/// RFC 8414 location for an issuer without a path (RFC 8414 §3).
pub const OAUTH_METADATA_PATH: &str = "/.well-known/oauth-authorization-server";

/// The scope that asks for a short-lived token carrying the user's elevated groups.
pub const ELEVATE_SCOPE: &str = "elevate";
/// Prefix of the group names elevation adds. Regular group names cannot contain `:`, so no
/// group can spell an elevated one.
pub const ELEVATED_GROUP_PREFIX: &str = "elevated:";
/// Prefix kube-apiserver puts before every Genkan username and group.
pub const IDENTITY_PREFIX: &str = "genkan:";
/// The group (and elevated group) of the first administrator. Installed RBAC lets
/// `genkan:admins` read the cluster and makes `genkan:elevated:admins` cluster admin.
pub const ADMIN_GROUP: &str = "admins";
