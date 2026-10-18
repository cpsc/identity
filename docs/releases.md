# Releases

Use [GitHub Releases](https://github.com/engawahq/genkan/releases) to download Genkan.
While Genkan is in alpha, releases are marked as prereleases.

## Choose your platform

| Platform | CLI archive | Container image |
|---|---|---|
| Linux amd64 / x86_64 | `genkanctl-v<VERSION>-linux-amd64.tar.gz` | `ghcr.io/engawahq/genkan:v<VERSION>` |
| Linux arm64 / aarch64 | `genkanctl-v<VERSION>-linux-arm64.tar.gz` | Same image tag; Docker selects your architecture |

The CLI and server are statically linked. For macOS, Windows or other architectures, build
the CLI from source.

Each release includes checksums, signatures and SBOMs. An SBOM is a list of the dependencies
included in an artifact; the downloadable SBOM files end in `.cdx.json`.

## Download and verify

You need GitHub CLI (`gh`), `cosign` 3.x and GNU `sha256sum`. Pulling the image also needs Docker.
These command-line tools, apart from Docker, are available in the repository's `nix develop` shell.

Choose a release version and your architecture, then download its files:

```sh
version=v0.0.1
arch=arm64 # use amd64 for x86_64
identity="https://github.com/engawahq/genkan/.github/workflows/release.yml@refs/tags/$version"
gh release download "$version" --repo engawahq/genkan --dir downloaded
```

Verify the signature on `SHA256SUMS`, then use it to check the downloaded files. The identity
ties the signature to Genkan's release workflow and the version you chose.

```sh
(
  set -e
  cd downloaded
  cosign verify-blob --bundle SHA256SUMS.sigstore.json \
    --certificate-identity "$identity" \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS
  sha256sum --check --strict SHA256SUMS
)
```

After both checks pass, extract and try the CLI:

```sh
tar -xzf "downloaded/genkanctl-$version-linux-$arch.tar.gz"
./genkanctl --help
```

To verify and pull the container, read its digest from the checked `image-digests.txt`.
A digest identifies the exact image bytes.

```sh
image=$(awk '$1 == "index" {print $2}' downloaded/image-digests.txt)
cosign verify --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com "$image"
cosign verify-attestation --type cyclonedx --certificate-identity "$identity" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com "$image"
docker pull --platform "linux/$arch" "$image"
```

The image and its platform variants are signed, with server SBOMs attached. The signed
checksums also cover the CLI archives, downloadable SBOMs, image digests and `provenance.json`,
which records the source commit and build run.

## Publish a release

1. Update the workspace version in `Cargo.toml` and update `Cargo.lock`. For the first release,
   use `0.0.1`. Add a `## v<VERSION>` section to `CHANGELOG.md` with the alpha status,
   platform scope and known limitations. Use level-three headings within each version;
   the publication script includes only the matching version section in the GitHub release notes.
   Merge the release commit to `main`.
2. In GitHub Actions, select **Release → Run workflow**, choose that commit's branch and enter
   the matching version, such as `v0.0.1`. This runs CI and tries both platform builds without
   publishing.
3. Make sure the GHCR package `engawahq/genkan` is public and this repository's Actions token
   can write to it. If the package is new, the first publishing run creates it; change its
   visibility before the download checks run. Set repository rulesets to protect `main` and
   restrict release-tag changes to maintainers.
   Enable GitHub private vulnerability reporting and confirm the reporting link in `SECURITY.md`
   works before making the release public.
4. Tag the release commit and push the tag. Replace `YOUR_RELEASE_COMMIT` with its commit hash:

   ```sh
   git tag v0.0.1 YOUR_RELEASE_COMMIT
   git push origin v0.0.1
   ```

5. Watch the **Release** run in GitHub Actions. A successful run leaves a **draft prerelease**;
   it never makes the GitHub release public automatically.
6. Download the draft assets with a maintainer's authenticated GitHub CLI:

   ```sh
   gh release download v0.0.1 --repo engawahq/genkan --dir downloaded
   ```

   Draft assets are not anonymously downloadable. For the download step in
   [Get started](get-started.md), transfer these exact assets to the clean test server and
   perform the same signature and checksum verification; do not rebuild the CLI or image.
   Complete the [installation checklist](release-installation.md) on Linux amd64 and arm64.
   Record the release commit, CLI checksums, image digests, cluster versions and observed
   outcomes in [#43](https://github.com/engawahq/genkan/issues/43). Keep acceptance failures
   visible; a green release workflow is not a substitute for this checklist.
7. Only after the workflow and installation acceptance pass, make the prerelease public:

   ```sh
   gh release edit v0.0.1 --repo engawahq/genkan --draft=false --prerelease
   ```

   Repeat the guide's unauthenticated downloads and signature/checksum verification against
   the public URLs. Announce the release only after those checks succeed.

The tag must match the Cargo version, and the commit must be on `main`. The workflow reruns
CI, builds both platforms using the pinned Rust/Nix toolchain and locked dependencies, and
tests the CLI archives and images it will publish. It then signs and uploads the artifacts,
downloads them on fresh runners, and verifies and runs them while the release stays a draft.
Signing uses GitHub's short-lived identity token through Sigstore. The publication job has
contents/package write and identity-token permissions. The download verification job also needs
`contents: write`: GitHub exposes draft releases only to tokens with push access, even in a
public repository. It does not publish or edit the release; making it public remains a separate
maintainer action.

Image tags are public while the GitHub release is still a draft. Do not treat an image tag or
a successful build as installation acceptance. Use the digest in the release notes when pinning
a deployment.

If publication succeeds but download verification fails, preserve the draft, tag and signed
artifacts. Fix the verification workflow on `main`, then run **Release → Run workflow** from
`main` with the existing version and `verify_only` enabled:

```sh
gh workflow run release.yml --ref main -f version=v0.0.1 -F verify_only=true
```

This mode skips CI/build/publication, checks out the existing version tag, and reruns native
amd64/arm64 signature, checksum and Kubernetes smoke verification against the existing assets.
It reads the image digest from release metadata; the verification script authenticates that
metadata and binds the image to it before pulling. It does not rebuild, re-sign, overwrite or
publish anything. Rerunning the original failed run would reuse its old workflow permissions,
so use a new dispatch of the corrected workflow instead.

If publication itself fails, inspect the logs and any partial draft or registry uploads before
deciding how to recover. Do not blindly rerun publication or move an existing version tag.

## Try the artifacts on a test machine

On a disposable Linux machine matching the chosen architecture, install Docker, check out
the release source and enter `nix develop`. With the variables and `downloaded` directory
from the examples above, run:

```sh
bash scripts/release-verify.sh "$version" "$arch" downloaded "$image" "$identity"
```

The script verifies the downloads, pulls the image and creates a temporary k3s cluster.
It uses the downloaded CLI to install Genkan, checks the running server and removes the
cluster when finished. It needs access to GHCR, Sigstore, the k3s image and Nix caches.

For a local build, follow [Building from source](../CONTRIBUTING.md#building-from-source).

Before publishing a new version, follow the [installation checklist](release-installation.md)
to check the guide, browser sign-in and account recovery with the release artifacts.
