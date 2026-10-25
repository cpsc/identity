# Managing users

Zeabur Identity stores people, passkeys and sessions as Kubernetes objects in its own namespace. You
manage them with two tools:

- `zeabur-identityctl` creates the one-time links people use to set up a passkey;
- `kubectl` (or your GitOps tool) does everything else: groups, admin accounts, signing people
  out, removing them.

The examples assume the cluster from [Get started](get-started.md): namespace `zeabur-identity`
and issuer `https://identity.example.test`. Run them where you have the cluster admin's kubeconfig,
and use your own issuer:

```sh
export ZEABUR_IDENTITY_ISSUER=https://identity.example.test
```

`zeabur-identityctl` reads the issuer from `ZEABUR_IDENTITY_ISSUER`, so the commands below do not repeat it.

## The objects

| Object | Short name | What it is | Who creates it |
|---|---|---|---|
| `User` | `zbuser` | a person: username, display name, groups, account type | you |
| `Credential` | `zbcred` | one passkey, or one linked GitHub or Google account | Zeabur Identity, when someone uses a link |
| `Invitation` | `zbinvite` | a pending one-time link | `zeabur-identityctl invite`; Zeabur Identity deletes it once used or expired |
| `Grant` | `zbgrant` | one signed-in session: an application's (`g-…`) or the account page's (`s-…`) | Zeabur Identity, at each sign-in |

Credentials, invitations and grants carry the label `identity.zeabur.com/user=<username>`, so
`-l identity.zeabur.com/user=alice` selects everything that belongs to `alice`.

The lifetimes below are the defaults: regular accounts' tokens last 10 minutes and admin
accounts' tokens 5 minutes. See [Sessions](#sessions) if you changed them.

## Add someone

```sh
zeabur-identityctl invite --username alice --display-name "Alice Chen" --group dev
```

This creates the user `alice` in the group `dev`, and prints a one-time link. Send the link to
Alice over a channel you trust. When she opens it, she creates a passkey, or links her
[GitHub](github-sign-in.md) or [Google](google-sign-in.md) account if you enabled them.

Things to know:

- **Anyone with the link can become Alice** until it is used or expires. Send it only to her.
  Issuing the link and using it each add an event to the `User` (`InvitationIssued`,
  `InvitationRedeemed`), so you can see when and from where it was used:

  ```sh
  kubectl -n zeabur-identity get events --field-selector involvedObject.name=alice
  ```
- **The link lasts 7 days, and no longer.** Use `--valid-for 24h`, for example, for a shorter
  window.
- **Options apply only to new users.** `--group`, `--admin` and `--display-name` set
  up a new user. For an existing username, the invitation only adds a passkey or a linked
  GitHub or Google account; it does not change groups or the name.
- **Only for someone who cannot sign in yet.** If the user already has a passkey or a linked
  account, `invite` stops with an error. `--reset` deletes them first; see
  [Lost passkey](#lost-passkey-account-recovery).
- **Repeat `--group`** to add several groups.
- **Zeabur Identity sends no mail.** The link goes to stdout and everything else to stderr, so a script
  can deliver it however your team works:

  ```sh
  link=$(zeabur-identityctl invite --username alice --group dev)
  mail -s "Set up your passkey" alice@example.com <<EOF
  Open this link on the device that will keep your passkey:

  $link
  EOF
  ```

Kubernetes sees Alice as the user `zeabur:alice` in the group `zeabur:dev`. Give permissions to
the group, not to the person. For example, to let the `dev` group edit everything in a `dev`
namespace:

```sh
kubectl create namespace dev            # if it does not exist yet
kubectl create rolebinding dev-edit --clusterrole=edit --group=zeabur:dev -n dev
```

### If the link does not work

Invitation links need the server's `--invitations` option. [Get started](get-started.md)
turns it on. If your installation does not have it, regenerate the deployment with it:

```sh
ZEABUR_IDENTITY_IMAGE=$(kubectl -n zeabur-identity get deployment zeabur-identity -o jsonpath='{.spec.template.spec.containers[0].image}')
zeabur-identityctl manifests --image "$ZEABUR_IDENTITY_IMAGE" \
  --serve-arg=--invitations | kubectl apply --server-side -f -
kubectl -n zeabur-identity rollout status deploy/zeabur-identity
```

`zeabur-identityctl manifests` does not remember earlier options. Pass the same image, namespace,
`--config-secret` and `--serve-arg` options you installed with, plus the new one.

## Change groups or the display name

Edit the `User`:

```sh
kubectl -n zeabur-identity patch zbuser alice --type=merge -p '{"spec":{"groups":["dev","oncall"]}}'
```

The change shows up in the next token Zeabur Identity issues. Tokens already issued keep the old groups
until they expire, within ten minutes for `kubectl`.

If a directory sync manages the user (label `identity.zeabur.com/source=directory`), change the
name and groups in the directory instead; see [Provisioning users](provisioning-users.md).

The group `admins` is refused in `groups`. It comes only from the account type; see
[Admin accounts](#admin-accounts).

You cannot rename a user: RBAC bindings and audit logs refer to the username. To rename
someone, create a new user and remove the old one.

## Admin accounts

A `User` is a **regular** account (the default) or an **admin** account
(`spec.accountType: Admin`). Only admin accounts' tokens carry the group `admins`, which
Kubernetes sees as `zeabur:admins`. The installation binds `zeabur:admins` to `cluster-admin`
(ClusterRoleBinding `zeabur-identity-cluster-admins`), so an admin acts with full rights at
once. There is no second sign-in.

The admin account itself is the safeguard:

- **Keep it separate.** Give each admin a regular account for everyday work too, and use the
  admin account only for cluster changes.
- **Passkey only.** An admin account cannot sign in with GitHub or Google, or link either. The
  sign-in page says "Use a passkey for this account".
- **Short sessions.** Admin tokens last 5 minutes, and with several teams or AAL3 an idle
  session ends after 15 minutes. See [Sessions](#sessions).

Regular accounts get only the rights you bind to their groups. Nothing is bound for them by
default.

### The first admin

`zeabur-identityctl setup-link` creates the first user as an admin account, with no groups, and
prints a one-time link for its passkey:

```sh
zeabur-identityctl setup-link --username admin
```

The same command with `--reset` recovers an admin who lost every passkey; see
[Lost passkey](#lost-passkey-account-recovery).

### Make or unmake an admin

To invite a new user as an admin account:

```sh
zeabur-identityctl invite --username alice --admin
```

To change an existing user:

```sh
zeabur-identityctl account-type alice admin     # or: regular
```

This also removes `admins` from the user's groups, if an older version put it there. With plain
`kubectl`:

```sh
kubectl -n zeabur-identity patch zbuser alice --type merge -p '{"spec":{"accountType":"Admin"}}'
kubectl -n zeabur-identity get zbuser            # the Type column shows the account type
```

The change applies at the user's next sign-in or token refresh. Access tokens already issued
stay valid until they expire, within 5 or 10 minutes. Making an account admin ends its GitHub
and Google sessions at their next refresh, because admin accounts use passkeys only.

Directory sync never makes or unmakes an admin account; see
[Provisioning users](provisioning-users.md).

## Sessions

Every sign-in starts a session. The application gets a short access token and a refresh token,
and renews the access token without a passkey until the session ends. A session ends when it is
idle too long, or at the latest 12 hours after the sign-in. The limits follow NIST SP 800-63B-4's
reauthentication guidance:

| Account | Access token | Session ends when idle for | Session ends at the latest |
|---|---|---|---|
| Regular | 10 minutes | 1 hour | 12 hours after sign-in |
| Admin, one team | 5 minutes | 1 hour | 12 hours after sign-in |
| Admin, several teams or AAL3 | 5 minutes | 15 minutes | 12 hours after sign-in |

The 12 hours count from the passkey, GitHub or Google sign-in, and a refresh does not reset
them. Zeabur Identity keeps no single sign-on session, so every authorization request shows the
sign-in page. The token response carries `refresh_expires_in`: the seconds until the refresh
token goes idle.

To change the limits, pass these options to `zeabur-identity` (with `--serve-arg` when you
generate the manifests):

| Option | Default | What it limits |
|---|---|---|
| `--token-lifetime` | `10m` | regular accounts' access and ID tokens |
| `--admin-token-lifetime` | `5m` | admin accounts' access and ID tokens |
| `--refresh-idle` | `1h` | idle time of a session |
| `--admin-refresh-idle` | `15m` | idle time of an admin session, with several teams or AAL3 |
| `--refresh-absolute` | `12h` | a session's whole life, from sign-in |

Token lifetimes must be between 1 minute and 1 hour. Idle limits cannot be longer than
`--refresh-absolute`. kube-apiserver also refuses tokens longer than the limit
`zeabur-identityctl kube-config --max-token-lifetime` set (`1h` by default).

### Install-wide settings

The ConfigMap `zeabur-identity-settings`, in Zeabur Identity's namespace, holds two settings:

| Key | Values | Default | What it does |
|---|---|---|---|
| `teams` | `single`, `multiple` | `single` | With `multiple`, idle admin sessions end after 15 minutes. |
| `admin-aal3` | `false`, `true` | `false` | With `true`, admin accounts need a device-bound passkey; see [AAL3 for admins](#aal3-for-admins). |

Every installation starts as a single team, because the installer cannot know. On a single-team
cluster the admin is often the only person, so admin sessions idle as long as regular ones.
When several teams share the cluster, say so:

```sh
kubectl -n zeabur-identity create configmap zeabur-identity-settings --from-literal=teams=multiple
```

To change a setting later:

```sh
kubectl -n zeabur-identity patch configmap zeabur-identity-settings --type merge -p '{"data":{"teams":"single"}}'
```

Zeabur's Settings page writes the same ConfigMap. Zeabur Identity reads it at every sign-in and
refresh, so no restart is needed. A missing ConfigMap or key gives the default. A value Zeabur
Identity does not understand gives the stricter setting (`multiple` or `true`) and a warning in
its log.

### AAL3 for admins

With `admin-aal3: "true"`, admin accounts accept only **device-bound** passkeys: security keys,
or passkeys that stay on one device. Synced passkeys, such as those in iCloud Keychain or Google
Password Manager, are refused at sign-in, when a passkey is added, and at refresh. Sessions that
started with a synced passkey end at their next refresh. The refusal says "This passkey syncs to
other devices. Admin accounts here need a security key, or a passkey that stays on one device."
Idle admin sessions end after 15 minutes, whatever `teams` says. Regular accounts do not change.

NIST SP 800-63B-4 allows synced passkeys up to AAL2 only. Device-bound passkeys can meet AAL3.

The authenticator itself reports whether a passkey can sync (the WebAuthn backup eligibility
flag, BE). Zeabur Identity asks for no attestation, so nothing verifies that report. AAL3 here
keeps synced passkeys out; it does not prove what hardware holds the key.

**Before you turn it on**, make sure every admin has a device-bound passkey. `BE` must be
`false`:

```sh
kubectl -n zeabur-identity get zbcred \
  -o custom-columns=NAME:.metadata.name,USER:.spec.user,BE:.spec.passkey.backupEligible
```

An admin without one is locked out until you run
`zeabur-identityctl setup-link --username <admin> --reset` and they enrol a security key from
the new link. Zeabur's Settings page refuses to turn AAL3 on unless the admin using it has a
device-bound passkey. Then turn it on:

```sh
kubectl -n zeabur-identity patch configmap zeabur-identity-settings --type merge -p '{"data":{"admin-aal3":"true"}}'
```

## The account page

People manage their own passkeys at `https://<issuer>/account`: add one, rename one, remove one,
and see and end their sessions. Adding or removing a passkey asks for a passkey confirmation
first. Nobody can remove their last passkey there. Removing a passkey also ends every session
that signed in with it. Each change is logged and recorded as an event on the `User`:

```sh
kubectl -n zeabur-identity get events --field-selector involvedObject.name=alice
```

An account-page session lasts an hour. Password managers find the page through
`/.well-known/passkey-endpoints`.

## Add another passkey or device

Most passkeys sync through the platform account (iCloud Keychain, Google Password Manager), so
a new phone usually needs nothing. For a separate security key, the person can add it on the
[account page](#the-account-page).

`invite` does not add a passkey or a linked account to a user who can already sign in. If a
link went to the wrong person, its holder could then sign in as that user. When someone cannot
sign in any more, follow [Lost passkey](#lost-passkey-account-recovery).

## Remove a passkey or linked account

```sh
kubectl -n zeabur-identity get zbcred -l identity.zeabur.com/user=alice
kubectl -n zeabur-identity delete zbcred <name>
```

The deleted credential cannot sign in again. Sessions it already started keep working until you
[sign them out](#sign-someone-out). (Removing it on the account page signs those out too.)

## Sign someone out

Each signed-in session is a `Grant`. Delete it to end the session:

```sh
kubectl -n zeabur-identity get zbgrant -l identity.zeabur.com/user=alice
kubectl -n zeabur-identity delete zbgrant -l identity.zeabur.com/user=alice
```

The session cannot renew its token any more. The token already in hand still works until it
expires, within ten minutes.

## Lost passkey (account recovery)

```sh
zeabur-identityctl setup-link --username alice --reset
```

This deletes all of Alice's credentials (passkeys and any linked GitHub or Google account),
sessions and pending invitations, and prints a new one-time link that lasts an hour. Whoever
opens the link becomes Alice, so confirm you are talking to her before you send it. The
`User` gets a `SetupLinkIssued` event now and a `SetupLinkRedeemed` event when the link is used. For a
regular account, `zeabur-identityctl invite --username alice --reset` does the same and prints
an invitation instead, which can also link GitHub or Google.

## Suspend or remove someone

**Suspend first.** A disabled user cannot sign in, and their sessions stop renewing. Nothing is
deleted, so you can undo it:

```sh
kubectl -n zeabur-identity patch zbuser alice --type=merge -p '{"spec":{"disabled":true}}'
```

**To remove someone for good**, delete the `User`:

```sh
kubectl -n zeabur-identity delete zbuser alice
```

Their passkeys, linked GitHub or Google accounts, sessions and invitation links then stop working.
Each one is bound to the UID of this `User`, and a new `User` with the same name gets a new UID.
The token already in hand still works until it expires, within ten minutes.

Kubernetes garbage collection deletes those objects soon after. To delete them at once, run this
too, before or after you delete the `User`:

```sh
for kind in zbcred zbgrant zbinvite; do
  kubectl -n zeabur-identity delete "$kind" -l identity.zeabur.com/user=alice
done
```

## Keep users in Git

`User` objects are plain manifests, so they can live in a GitOps repository:

```yaml
apiVersion: identity.zeabur.com/v1alpha1
kind: User
metadata:
  name: alice
  namespace: zeabur-identity
spec:
  username: alice
  displayName: Alice Chen
  groups: [dev]
  # accountType: Admin   # for an admin account
```

Keep `Credential`, `Grant` and `Invitation` out of Git. Zeabur Identity creates and deletes them, and a
sync tool that prunes them would remove people's passkeys or sign them out.

To keep users in step with a company directory instead, see
[Provisioning users](provisioning-users.md).
