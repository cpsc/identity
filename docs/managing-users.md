# Managing users

Zeabur Identity stores people, passkeys and sessions as Kubernetes objects in its own namespace. You
manage them with two tools:

- `zeabur-identityctl` creates the one-time links people use to set up a passkey;
- `kubectl` (or your GitOps tool) does everything else: groups, admin rights, signing people
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
| `User` | `zbuser` | a person: username, display name, groups | you |
| `Credential` | `zbcred` | one passkey, or one linked GitHub or Google account | Zeabur Identity, when someone uses a link |
| `Invitation` | `zbinvite` | a pending one-time link | `zeabur-identityctl invite`; Zeabur Identity deletes it once used or expired |
| `Grant` | `zbgrant` | one signed-in session: an application's (`g-…`) or the account page's (`s-…`) | Zeabur Identity, at each sign-in |

Credentials, invitations and grants carry the label `identity.zeabur.com/user=<username>`, so
`-l identity.zeabur.com/user=alice` selects everything that belongs to `alice`.

The lifetimes below are the defaults: everyday tokens last 10 minutes and admin tokens 5
minutes. See [token limits](../crates/zeabur-identityctl/README.md#configure-token-limits-and-certificates)
if you changed them.

## Add someone

```sh
zeabur-identityctl invite --username alice --display-name "Alice Chen" --group dev
```

This creates the user `alice` in the group `dev`, and prints a one-time link. Send the link to
Alice over a channel you trust. When she opens it, she creates a passkey, or links her
[GitHub](github-sign-in.md) or [Google](google-sign-in.md) account if you enabled them.

Things to know:

- **Anyone with the link can become Alice** until it is used or expires. Send it only to her.
- **The link lasts 7 days.** Use `--valid-for 24h`, for example, for a shorter window.
- **Options apply only to new users.** `--group`, `--elevated-group` and `--display-name` set
  up a new user. For an existing username, the invitation only adds a passkey or a linked
  GitHub or Google account; it does not change groups or the name.
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

You cannot rename a user: RBAC bindings and audit logs refer to the username. To rename
someone, create a new user and remove the old one.

## Admin rights

Everyday tokens can only do what the user's ordinary groups allow. For powerful roles, give
the user an **elevated group**:

```sh
kubectl -n zeabur-identity patch zbuser alice --type=merge -p '{"spec":{"elevatedGroups":["admins"]}}'
```

Alice gets the elevated group only when she asks for it, for example through the admin
`kubectl` context, and signs in with her passkey again. Kubernetes then sees her in `zeabur:elevated:admins`, for five
minutes. Her everyday tokens never carry it.

The installation binds `zeabur:elevated:admins` to `cluster-admin`. Bind other elevated groups
to narrower roles in the same way, for example `--group=zeabur:elevated:dev-admins`.

Directory sync never touches elevated groups, so this works the same for synced users.

### Which applications may ask for admin rights

Only clients registered with `allowElevation: true` can ask. Others get an `invalid_scope`
error. Turn it on for the client you administer the cluster with, such as `kubectl` in
[Get started](get-started.md), and leave it off for web applications that only sign people in:

```sh
kubectl -n zeabur-identity patch zbclient kubectl --type=merge -p '{"spec":{"allowElevation":true}}'
```

Turning it off takes effect at once, even for sign-ins in progress. When an application asks
for admin rights, the sign-in page says so and shows for how long.

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
[account page](#the-account-page). To link a GitHub or Google account, or when they cannot sign
in, invite the same username again. The new link adds one passkey or linked account and
leaves everything else as it is:

```sh
zeabur-identityctl invite --username alice
```

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
opens the link becomes Alice, so confirm you are talking to her before you send it.

## Suspend or remove someone

**Suspend first.** A disabled user cannot sign in, and their sessions stop renewing. Nothing is
deleted, so you can undo it:

```sh
kubectl -n zeabur-identity patch zbuser alice --type=merge -p '{"spec":{"disabled":true}}'
```

**To remove someone for good**, delete their credentials, sessions and invitations first, then
the `User`:

```sh
for kind in zbcred zbgrant zbinvite; do
  kubectl -n zeabur-identity delete "$kind" -l identity.zeabur.com/user=alice
done
kubectl -n zeabur-identity delete zbuser alice
```

> [!WARNING]
> Keep this order. Deleting only the `User` leaves the old credentials (passkeys and linked GitHub
> or Google accounts) around for a while: until Kubernetes garbage collection removes them, or
> forever for objects created by hand without an owner reference. If you create a new `User` with
> the same name in the meantime, the old credentials sign in as that new user.

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
```

Keep `Credential`, `Grant` and `Invitation` out of Git. Zeabur Identity creates and deletes them, and a
sync tool that prunes them would remove people's passkeys or sign them out.

To keep users in step with a company directory instead, see
[Provisioning users](provisioning-users.md).
