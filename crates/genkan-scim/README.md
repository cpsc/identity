# genkan-scim

SCIM 2.0 provisioning ([RFC 7643], [RFC 7644]) for Genkan: an identity provider creates,
updates, deactivates and deletes users, and manages their membership in a fixed set of groups.
The routes are served under `/scim/v2` by the `genkan` server when it is started with
`--scim-token-file`.

## Contents

- [`router`](crate::router) and [`Scim`](crate::Scim): the HTTP endpoints over any
  `genkan_store::scim::UserAdmin`.
- [`model`](crate::model): the `User` and `Group` JSON, list responses, errors, `eq` filters
  and PATCH operations.

Supported: `/ServiceProviderConfig`, `/ResourceTypes`, `/Schemas`, `/Users` and `/Groups` with
GET, POST, PUT, PATCH and DELETE, `eq` filters on `userName`, `externalId` and `displayName`, and
`startIndex`/`count` pagination (at most 200 per page). `active: false` disables a user. Not
supported: bulk operations, sorting, entity tags.

Groups are not stored: a group is the set of users whose `groups` contain its name, and only
names on the allowlist exist. Membership changes are written user by user with
compare-and-swap, retried when another write gets there first.

## Place in the workspace

Depends on `genkan-core` and `genkan-store`. Used by the `genkan` server, which passes the
`genkan-k8s` implementation (`User` objects labelled `genkan.engawahq.dev/source=scim`).

## Key properties

- **SCIM sees only its own users.** The bootstrap admin and users created by hand or by
  `genkanctl invite` are invisible to it, and a `userName` they hold cannot be taken.
- **No administrative rights.** Groups come from an allowlist (`--scim-group`) and elevated
  groups are never assigned. The admission policy installed with SCIM (`genkanctl manifests
  --scim`) enforces both rules on Genkan's service account as well, so a bug here cannot break
  them.
- **Static bearer token** ([RFC 6750]), at least 32 characters, kept only as a SHA-256 hash and
  compared in constant time.
- **Provisioning is not enrolment.** A user created by SCIM still enrols a passkey or links an
  account through an invitation.

## References

- [RFC 7643] P. Hunt, Ed., K. Grizzle, E. Wahlstroem, and C. Mortimore, "System for
  Cross-domain Identity Management: Core Schema," RFC 7643, IETF, Sep. 2015.
- [RFC 7644] P. Hunt, Ed., K. Grizzle, M. Ansari, E. Wahlstroem, and C. Mortimore, "System for
  Cross-domain Identity Management: Protocol," RFC 7644, IETF, Sep. 2015.
- [RFC 6750] M. Jones and D. Hardt, "The OAuth 2.0 Authorization Framework: Bearer Token
  Usage," RFC 6750, IETF, Oct. 2012.

[RFC 7643]: https://www.rfc-editor.org/rfc/rfc7643
[RFC 7644]: https://www.rfc-editor.org/rfc/rfc7644
[RFC 6750]: https://www.rfc-editor.org/rfc/rfc6750
