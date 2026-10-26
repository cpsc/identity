# Get started: k3s on an Ubuntu server, kubectl with a passkey

By the end of this guide, you will sign in to a k3s cluster with a passkey and run
`kubectl` from your computer.

You will download Zeabur Identity, install it on an Ubuntu server, then set up HTTPS and your first
admin account. The server uses the Traefik ingress included with k3s.

The guide talks about two machines:

- **Your computer** runs the browser and `kubectl`. It can run macOS, Linux or Windows. The
  commands are written for a POSIX shell; on Windows, use WSL.
- **The server** runs k3s and Zeabur Identity. Any Ubuntu machine your computer can reach over the
  network works: a VPS, a homelab box, a spare PC or a local VM.

The cluster's admin kubeconfig stays on the server, where you use it to install things and to
get back in if Zeabur Identity breaks; keep an encrypted copy offline too. Your computer gets the
cluster's CA certificate and a kubeconfig that signs in through Zeabur Identity. Every request it sends
carries a token that expires within minutes.

```text
Your computer                               server (k3s)
 browser ──https://identity.example.test──▶ Traefik :443 ──▶ zeabur-identity (zeabur-identity)
 kubectl ──https://identity.example.test:6443──▶ kube-apiserver
                                              │ verifies tokens itself; discovery and keys
                                              └─▶ https://<zeabur-identity ClusterIP> (internal TLS)
```

The examples use these names. Replace them with yours as you go:

| Name | In the examples |
|---|---|
| The server's host name | `identity.example.test` |
| Issuer (Zeabur Identity's public URL) | `https://identity.example.test` |
| SSH login to the server | `ubuntu@identity.example.test`, a user with `sudo` |

The host name must lead your computer to the server. With a domain of your own, add a DNS
record. Names under `.test` never resolve on the internet, so for those add a line to your
hosts file instead (`/etc/hosts`, or `C:\Windows\System32\drivers\etc\hosts` on Windows).
WSL copies the Windows hosts file when it starts, so after editing it run `wsl --shutdown` in
Windows and open a new WSL terminal.

## What you need

On **your computer**:

- `kubectl`;
- [kubelogin](https://github.com/int128/kubelogin), the `kubectl` plugin that opens the browser
  to sign in. Install it with Homebrew (`brew install int128/kubelogin/kubelogin`), Nix
  (`nixpkgs#kubelogin-oidc`), or [krew](https://krew.sigs.k8s.io/docs/user-guide/setup/install/)
  (`kubectl krew install oidc-login`, after installing krew itself);
- [mkcert](https://github.com/FiloSottile/mkcert), with its local CA trusted where your browser
  runs: run `mkcert -install` once (it may ask for your password). Firefox also needs NSS's
  `certutil`; see mkcert's README. With WSL, the browser runs on Windows, so install the
  Windows build (for example `winget install FiloSottile.mkcert`) and call it as `mkcert.exe`
  from WSL, both here and in step 4.
- `ssh` access to the server.

On **the server**: Ubuntu 24.04 or later on amd64 or arm64, 4 CPUs, 8 GiB memory and
30 GiB free disk. Install `curl` and [cosign 3.x](https://docs.sigstore.dev/cosign/system_config/installation/).
Allow your computer to reach TCP ports 443 (sign-in) and 6443 (Kubernetes API).
Use a spare machine or VM: this guide installs k3s and changes its configuration.

Keep two terminals open: one on your computer, and one logged in to the server. Each step says
which one to use. In the terminal on your computer, set the SSH login so later commands can use
it:

```sh
SERVER=ubuntu@identity.example.test
```

## 1. Download Zeabur Identity

Log in to the server:

```sh
ssh "$SERVER"
```

Choose a version from [GitHub Releases](https://github.com/zeabur-oss/identity/releases).
The examples use `v0.1.0`. Set `arch` to `amd64` for x86_64 or `arm64` for aarch64:

```sh
version=v0.1.0
arch=arm64
mkdir -p ~/zeabur-identity-download
```

Download the CLI, its signed checksums and the image digests. This block verifies the files
before installing `zeabur-identityctl`:

```sh
(
  set -eu
  cd ~/zeabur-identity-download
  url="https://github.com/zeabur-oss/identity/releases/download/$version"
  archive="zeabur-identityctl-$version-linux-$arch.tar.gz"
  for file in "$archive" SHA256SUMS SHA256SUMS.sigstore.json image-digests.txt; do
    curl --fail --location "$url/$file" --output "$file"
  done
  identity="https://github.com/zeabur-oss/identity/.github/workflows/release.yml@refs/tags/$version"
  cosign verify-blob --bundle SHA256SUMS.sigstore.json \
    --certificate-identity "$identity" \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS
  sha256sum --check --strict --ignore-missing SHA256SUMS
  tar -xzf "$archive"
  sudo install -m 0755 zeabur-identityctl /usr/local/bin/zeabur-identityctl
  zeabur-identityctl --version
)
```

Now verify the image and its SBOM (the list of dependencies shipped in it). `ZEABUR_IDENTITY_IMAGE`
holds the release's exact image digest, which you will use when installing:

```sh
ZEABUR_IDENTITY_IMAGE=$(awk '$1 == "index" {print $2}' ~/zeabur-identity-download/image-digests.txt)
export ZEABUR_IDENTITY_IMAGE
identity="https://github.com/zeabur-oss/identity/.github/workflows/release.yml@refs/tags/$version"
cosign verify --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com "$ZEABUR_IDENTITY_IMAGE"
cosign verify-attestation --type cyclonedx --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com "$ZEABUR_IDENTITY_IMAGE"
```

Continue once both commands succeed. See [Releases](releases.md) for the platform list and
the other files included in a release.

## 2. Install k3s

> [!IMPORTANT]
> Step 5 changes how kube-apiserver checks who you are. Keep the admin kubeconfig below and
> SSH access so you can recover. Read
> [Backup, recovery and emergency access](operations.md) before that step.

Run the following on the server. It adds your host name to the API server's certificate and
sets up an empty authentication file. Step 5 fills that file with Zeabur Identity's settings.

```sh
sudo mkdir -p /etc/rancher/k3s/zeabur-identity /etc/rancher/k3s/config.yaml.d
sudo tee /etc/rancher/k3s/zeabur-identity/authn.yaml >/dev/null <<'EOF'
apiVersion: apiserver.config.k8s.io/v1
kind: AuthenticationConfiguration
anonymous:
  enabled: false
jwt: []
EOF
sudo tee /etc/rancher/k3s/config.yaml >/dev/null <<'EOF'
# The name your computer uses for the API server.
tls-san: [identity.example.test]
EOF
sudo tee /etc/rancher/k3s/config.yaml.d/50-zeabur-identity.yaml >/dev/null <<'EOF'
kube-apiserver-arg:
  - "authentication-config=/etc/rancher/k3s/zeabur-identity/authn.yaml"
EOF
curl -sfL https://get.k3s.io | sudo INSTALL_K3S_VERSION=v1.36.5+k3s1 sh -

install -d -m 700 ~/.kube
sudo install -o "$USER" -m 600 /etc/rancher/k3s/k3s.yaml ~/.kube/config
export KUBECONFIG="$HOME/.kube/config"
```

`~/.kube/config` is the cluster-admin credential. Use it to install things and to get back in
when Zeabur Identity is down, not for everyday work. Keep it on the server, and keep an encrypted copy
offline as described in
[Keep a way in that doesn't need Zeabur Identity](operations.md#keep-a-way-in-that-doesnt-need-zeabur-identity).

Check that the node and k3s's built-in ingress are ready:

```sh
kubectl wait --for=condition=Ready node --all --timeout=180s
kubectl -n kube-system wait --for=create deployment/traefik --timeout=180s
kubectl -n kube-system rollout status deployment/traefik --timeout=180s
```

Install [cert-manager](https://cert-manager.io/docs/installation/kubectl/). In step 5 it issues
and renews the certificate kube-apiserver uses to reach Zeabur Identity:

```sh
kubectl apply --server-side -f https://github.com/cert-manager/cert-manager/releases/download/v1.21.3/cert-manager.yaml
kubectl -n cert-manager wait deployment --all --for=condition=Available --timeout=300s
```

## 3. Install Zeabur Identity

Run this in the same server shell:

```sh
export ZEABUR_IDENTITY_ISSUER=https://identity.example.test
zeabur-identityctl crds | kubectl apply --server-side -f -
kubectl wait --for=condition=Established --timeout=90s \
  crd/users.identity.zeabur.com crd/credentials.identity.zeabur.com \
  crd/oauthclients.identity.zeabur.com crd/grants.identity.zeabur.com \
  crd/invitations.identity.zeabur.com
zeabur-identityctl manifests --image "$ZEABUR_IDENTITY_IMAGE" \
  --serve-arg=--invitations | kubectl apply --server-side -f -
kubectl -n zeabur-identity rollout status deploy/zeabur-identity --timeout=180s
```

The generated manifests include RBAC, the Service and the Deployment. Everything Zeabur Identity
owns lives in `zeabur-identity`. `--invitations` enables the links you will use to
[invite people](managing-users.md#add-someone).

Keep using this server shell. If you open a new one, restore these settings:

```sh
export KUBECONFIG="$HOME/.kube/config"
export ZEABUR_IDENTITY_ISSUER=https://identity.example.test
ZEABUR_IDENTITY_IMAGE=$(kubectl -n zeabur-identity get deployment zeabur-identity -o jsonpath='{.spec.template.spec.containers[0].image}')
export ZEABUR_IDENTITY_IMAGE
```

## 4. Publish the sign-in page over HTTPS

Browsers need a trusted HTTPS connection to create passkeys. This guide uses mkcert for
your test machine. For a public service, use a certificate from a public CA, for example
through [cert-manager](https://cert-manager.io/docs/).

On your computer, issue a certificate for the issuer's host name and copy it to the server. Only the
certificate and its key leave your computer; the CA's key stays. With WSL, write `mkcert.exe`
instead of `mkcert`.

```sh
mkcert -cert-file zeabur-identity.crt -key-file zeabur-identity.key identity.example.test
ssh "$SERVER" 'install -d -m 700 ~/zeabur-identity-tls'
scp zeabur-identity.crt zeabur-identity.key "$SERVER":zeabur-identity-tls/
rm zeabur-identity.key
```

On the server, store the certificate as a Secret and create an Ingress. K3s's Traefik
accepts HTTPS on port 443 and forwards requests to Zeabur Identity:

```sh
kubectl -n zeabur-identity create secret tls zeabur-identity-tls \
  --cert=$HOME/zeabur-identity-tls/zeabur-identity.crt --key=$HOME/zeabur-identity-tls/zeabur-identity.key
kubectl apply -f - <<'EOF'
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: zeabur-identity
  namespace: zeabur-identity
  annotations:
    traefik.ingress.kubernetes.io/router.entrypoints: websecure
    traefik.ingress.kubernetes.io/router.tls: "true"
spec:
  ingressClassName: traefik
  tls:
  - hosts: [identity.example.test]
    secretName: zeabur-identity-tls
  rules:
  - host: identity.example.test
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: zeabur-identity
            port: {number: 80}
EOF
```

Give Traefik a few seconds to load the route and certificate, then check from your computer.
You should get a JSON document whose `issuer` is `https://identity.example.test`:

```sh
curl --cacert "$(mkcert -CAROOT)/rootCA.pem" \
  https://identity.example.test/.well-known/openid-configuration
```

With WSL and the Windows mkcert, `mkcert -CAROOT` prints a Windows path. Convert it first:

```sh
curl --cacert "$(wslpath "$(mkcert.exe -CAROOT | tr -d '\r')")/rootCA.pem" \
  https://identity.example.test/.well-known/openid-configuration
```

### Before going public: rate-limit sign-in

For a public server, limit requests to the sign-in endpoints by client IP address.
You can skip this section on a server only you can reach.

First, preserve each visitor's address through k3s's load balancer. On the server, set
Traefik's `externalTrafficPolicy` to `Local` so visitors get separate rate-limit counters:

```sh
kubectl apply -f - <<'EOF'
apiVersion: helm.cattle.io/v1
kind: HelmChartConfig
metadata: {name: traefik, namespace: kube-system}
spec:
  valuesContent: |-
    service:
      spec:
        externalTrafficPolicy: Local
EOF
```

This uses k3s's [Traefik configuration](https://docs.k3s.io/helm/#customizing-packaged-components-with-helmchartconfig).
On a cluster with several nodes, `Local` means only nodes with a Traefik pod accept traffic.

Then add a rate limit to the sign-in routes:

```sh
kubectl apply -f - <<'EOF'
apiVersion: traefik.io/v1alpha1
kind: Middleware
metadata: {name: zeabur-identity-rate-limit, namespace: zeabur-identity}
spec:
  rateLimit:
    average: 10
    period: 1s
    burst: 20
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: zeabur-identity-rate-limited
  namespace: zeabur-identity
  annotations:
    traefik.ingress.kubernetes.io/router.entrypoints: websecure
    traefik.ingress.kubernetes.io/router.tls: "true"
    traefik.ingress.kubernetes.io/router.middlewares: zeabur-identity-zeabur-identity-rate-limit@kubernetescrd
spec:
  ingressClassName: traefik
  tls:
  - hosts: [identity.example.test]
    secretName: zeabur-identity-tls
  rules:
  - host: identity.example.test
    http:
      paths:
      - {path: /authorize, pathType: Exact, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /token, pathType: Exact, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /par, pathType: Exact, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /revoke, pathType: Exact, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /login, pathType: Prefix, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /setup, pathType: Prefix, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
      - {path: /upstream, pathType: Prefix, backend: {service: {name: zeabur-identity, port: {number: 80}}}}
EOF
```

This route takes precedence over the catch-all route for the listed paths. Discovery and keys
stay unlimited, because kube-apiserver and every client fetch them.

To check it, send 60 requests at once from your computer. About 20 get `200`, the rest
`429 Too Many Requests`:

```sh
ca="$(mkcert -CAROOT)/rootCA.pem"
for i in $(seq 1 60); do
  curl -s -o /dev/null -w '%{http_code}\n' --cacert "$ca" https://identity.example.test/login &
done | sort | uniq -c
```

Treat the numbers as a starting point:

- People behind one NAT share one address, and so share one limit. Raise it if your office
  signs in at the same time each morning.
- If another proxy sits in front of Traefik, configure Traefik to trust forwarded headers from
  that proxy only. Otherwise everyone shares one bucket, or a forged header gets around it.
- Each Traefik replica keeps its own count unless you set up distributed rate limiting.

See Traefik's [RateLimit options][traefik-ratelimit] for the details.

[traefik-ratelimit]: https://doc.traefik.io/traefik/reference/routing-configuration/http/middlewares/ratelimit/

## 5. Let kube-apiserver trust Zeabur Identity

First make sure you can get back in without Zeabur Identity. On the server, check that the admin
kubeconfig from step 2 works:

```sh
sudo k3s kubectl --kubeconfig=/etc/rancher/k3s/k3s.yaml auth whoami
sudo k3s kubectl --kubeconfig=/etc/rancher/k3s/k3s.yaml get --raw=/readyz
```

If either fails, stop here and fix it. Save an encrypted copy offline if you haven't yet.

kube-apiserver checks Zeabur Identity's tokens itself. To do that, it fetches Zeabur Identity's signing keys over
an internal HTTPS connection. `zeabur-identityctl kube-config` has cert-manager issue the
certificate for that connection and prints the configuration kube-apiserver needs. It also prints k3s setup
instructions, including a one-time restart; step 2 already did that, so you can skip them.

kube-apiserver checks the file about once a minute. To know when it has picked up your change,
count its successful reloads before and after. Define two small helpers in the server shell:

```sh
# Successful reloads of the authentication configuration so far.
reloads() {
  kubectl get --raw /metrics |
    awk '/^apiserver_authentication_config_controller_automatic_reloads_total/ &&
      /status="success"/ { n += $NF } END { print n + 0 }'
}
# Waits up to three minutes for the count to pass $1; prints the k3s log if it never does.
wait_for_reload() {
  for _ in $(seq 1 36); do
    [ "$(reloads)" -gt "$1" ] && return 0
    sleep 5
  done
  echo "Authentication configuration did not reload; inspect k3s logs." >&2
  sudo journalctl -u k3s --since "5 minutes ago" --no-pager
  return 1
}
```

Then write the new configuration to a temporary file and move it into place in one step, so
kube-apiserver never reads a half-written file, and wait for the reload:

```sh
before=$(reloads)
zeabur-identityctl kube-config --distro k3s > ~/authn.yaml &&
  sudo install -m 600 ~/authn.yaml /etc/rancher/k3s/zeabur-identity/.authn.yaml.tmp &&
  sudo mv /etc/rancher/k3s/zeabur-identity/.authn.yaml.tmp /etc/rancher/k3s/zeabur-identity/authn.yaml &&
  wait_for_reload "$before"
```

cert-manager renews the internal certificate by itself. See
[Check the internal certificate](#check-the-internal-certificate).

## 6. Register kubectl and create the first passkey

Every application that signs in through Zeabur Identity is registered as a client. Register `kubectl`
(through kubelogin, which receives the sign-in result on `127.0.0.1`):

```sh
zeabur-identityctl client create kubectl --type public \
  --redirect-uri http://127.0.0.1:8000/ --audience kubernetes | kubectl apply -f -
```

Then get a one-time link for the first admin:

```sh
zeabur-identityctl setup-link --username admin
```

This creates the user `admin` as an **admin account**: it signs in with a passkey only, and
Kubernetes gives it full rights (`cluster-admin`). See
[Admin accounts](managing-users.md#admin-accounts).

Open the printed link in the browser on your computer and create a passkey. The link works
once and expires after 60 minutes; run the command again if you need a new one.

## 7. Set up kubectl on your computer

Copy the cluster's CA certificate from the server. It is public; it only lets `kubectl` check that
it talks to the right API server.

```sh
CA=$(ssh "$SERVER" "grep certificate-authority-data ~/.kube/config | awk '{print \$2}'")
```

Then write a kubeconfig. kubelogin signs you in once and then renews the token quietly with a
refresh token, until the session ends:

```sh
mkdir -p ~/.kube
cat > ~/.kube/zeabur-identity-k3s.yaml <<EOF
apiVersion: v1
kind: Config
clusters:
- name: zeabur-identity-k3s
  cluster:
    server: https://identity.example.test:6443
    certificate-authority-data: $CA
users:
- name: passkey
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1
      interactiveMode: IfAvailable
      command: kubectl
      args: [oidc-login, get-token,
        --oidc-issuer-url=https://identity.example.test,
        --oidc-client-id=kubectl,
        --oidc-use-access-token,
        --listen-address=127.0.0.1:8000,
        --oidc-redirect-url=http://127.0.0.1:8000/]
contexts:
- {name: zeabur-identity-k3s, context: {cluster: zeabur-identity-k3s, user: passkey}}
current-context: zeabur-identity-k3s
EOF
```

Krew and Nix install kubelogin as a `kubectl` plugin, so the file works as written. If you
installed it with Homebrew, set `command: kubelogin` and drop `oidc-login` from `args`.

With WSL, kubelogin runs inside Linux and does not see the CA that the Windows mkcert trusted.
Add the CA to `args`, using the path from the WSL check in step 4:

```sh
wslpath "$(mkcert.exe -CAROOT | tr -d '\r')"   # e.g. /mnt/c/Users/you/AppData/Local/mkcert
```

```yaml
        --certificate-authority=/mnt/c/Users/you/AppData/Local/mkcert/rootCA.pem,
```

kubelogin cannot open the Windows browser from WSL either. Instead it prints
`Please visit the following URL in your browser manually: http://127.0.0.1:8000/`. Open that
address in your browser, and the sign-in continues as usual.

Keep `--oidc-use-access-token`: kube-apiserver accepts only Zeabur Identity's access tokens, not ID
tokens.

kubelogin keeps the refresh token in `~/.kube/cache/oidc-login`. To keep it in the system
keychain instead, add `--token-cache-storage=keyring`. With `--token-cache-storage=none`,
kubelogin keeps nothing, and each `kubectl` command opens the sign-in page.

## 8. Try it

```sh
export KUBECONFIG=~/.kube/zeabur-identity-k3s.yaml
kubectl auth whoami                      # opens the browser; user zeabur:admin, group zeabur:admins
kubectl get pods -A                      # allowed: zeabur:admins is bound to cluster-admin
kubectl -n default create configmap zeabur-identity-permission-check --from-literal=check=ok
kubectl -n default delete configmap zeabur-identity-permission-check
```

What just happened:

- **Admin tokens** last 5 minutes and renew without a prompt. You sign in again 12 hours after
  the first sign-in, or after an hour without using `kubectl` (15 minutes if several teams share
  the cluster). Regular accounts' tokens last 10 minutes. See
  [Sessions](managing-users.md#sessions).
- **Use the admin account for admin work only.** For everyday work, invite yourself a regular
  account and bind the rights you need to its groups; see [Manage users](managing-users.md).
- **Each token is traceable.** `kubectl auth whoami` shows `identity.zeabur.com/jti` and
  `identity.zeabur.com/client-id`. They also appear in Kubernetes audit events, so each request maps to one sign-in.
- **People, passkeys and sessions are Kubernetes objects** in `zeabur-identity`: a passkey is a
  `Credential`, and a signed-in session is a `Grant`. On the server, list them with
  `kubectl -n zeabur-identity get users,credentials,grants`.
  [Managing users](managing-users.md) explains how to work with them.

## Next

- [Back up, recover and upgrade Zeabur Identity](operations.md), and get back in when Zeabur Identity or the
  sign-in page is down.
- [Manage users](managing-users.md): invite people, add passkeys, sign people out or remove
  them.
- [Sign in with GitHub](github-sign-in.md) or [Google](google-sign-in.md) as an alternative
  to the passkey for everyday access.
- [Register more clients](clients.md), such as Grafana or Argo CD, so they sign people in
  through Zeabur Identity too.

## Check the internal certificate

cert-manager renews the certificate from step 5 30 days before it expires, every 90 days by
default. The configuration in `authn.yaml` names the root CA, which lasts 10 years, so it does not
change when the certificate is renewed. Check both certificates on the server:

```sh
kubectl -n zeabur-identity get certificate
```

Both lines must show `READY` `True`. If one does not, read the reason with
`kubectl -n zeabur-identity describe certificate <name>`. To replace the root CA, see
[Internal certificate](operations.md#internal-certificate).

## Troubleshooting

- **The browser shows a certificate error, or offers no passkey:** mkcert's CA is not trusted
  where the browser runs (`mkcert -install`), or the certificate's name does not match the
  issuer's host name.
- **`kubectl` says `Unauthorized`:** kube-apiserver may not have loaded the configuration from
  step 5. If it finds an error in the file, it ignores the file until it changes. On k3s, look
  for the reason with `sudo journalctl -u k3s | grep -i "authentication config"`.
- **Port 8000 is in use:** kubelogin listens there for the browser's redirect. Change
  `--listen-address` and `--oidc-redirect-url` together; any port on `127.0.0.1` matches the
  registered redirect URI.

## Clean up

On the server, uninstall k3s. This removes the cluster, Zeabur Identity and everything else installed
in it:

```sh
sudo /usr/local/bin/k3s-uninstall.sh
rm -rf ~/zeabur-identity-tls ~/authn.yaml ~/zeabur-identity-download
```

Then, on your computer:

```sh
rm ~/.kube/zeabur-identity-k3s.yaml
rm -rf ~/.kube/cache/oidc-login
```
