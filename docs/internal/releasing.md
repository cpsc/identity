# Releasing


1. Update the workspace version in `Cargo.toml` and update `Cargo.lock`. For the first release,
   use `0.1.0`. Add a `## v<VERSION>` section to `CHANGELOG.md` with the alpha status,
   platform scope and known limitations. Use level-three headings within each version;
   the publication script includes only the matching version section in the GitHub release notes.
   Merge the release commit to `main`.
2. In GitHub Actions, select **Release → Run workflow**, choose that commit's branch and enter
   the matching version, such as `v0.1.0`. This runs CI and tries both platform builds without
   publishing.
3. Make sure the GHCR package `zeabur-oss/identity` is public and this repository's Actions token
   can write to it. If the package is new, the first publishing run creates it; change its
   visibility before the download checks run. Set repository rulesets to protect `main` and
   restrict release-tag changes to maintainers.
   Enable GitHub private vulnerability reporting and confirm the reporting link in `SECURITY.md`
   works before making the release public.
4. Tag the release commit and push the tag. Replace `YOUR_RELEASE_COMMIT` with its commit hash:

   ```sh
   git tag v0.1.0 YOUR_RELEASE_COMMIT
   git push origin v0.1.0
   ```

5. Watch the **Release** run in GitHub Actions. A successful run leaves a **draft prerelease**;
   it never makes the GitHub release public automatically.
6. Download the draft assets with a maintainer's authenticated GitHub CLI:

   ```sh
   gh release download v0.1.0 --repo zeabur-oss/identity --dir downloaded
   ```

   Draft assets are not anonymously downloadable. For the download step in
   [Get started](../get-started/quickstart.mdx), transfer these exact assets to the clean test server and
   perform the same signature and checksum verification; do not rebuild the CLI or image.
   Complete the [installation checklist](release-check.md) on Linux amd64 and arm64.
   Record the release commit, CLI checksums, image digests, cluster versions and observed
   outcomes in the release tracking issue. Keep acceptance failures
   visible; a green release workflow is not a substitute for this checklist.
7. Only after the workflow and installation acceptance pass, make the prerelease public:

   ```sh
   gh release edit v0.1.0 --repo zeabur-oss/identity --draft=false --prerelease
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
gh workflow run release.yml --ref main -f version=v0.1.0 -F verify_only=true
```

This mode skips CI/build/publication and uses the existing tag's pinned Nix environment with
verification scripts from the workflow commit. It reruns native amd64/arm64 signature, checksum
and Kubernetes smoke verification against the existing assets, allowing verification-tool
fixes without changing the release tag or artifacts.
It reads the image digest from release metadata; the verification script authenticates that
metadata and binds the image to it before pulling. It does not rebuild, re-sign, overwrite or
publish anything. Rerunning the original failed run would reuse its old workflow permissions,
so use a new dispatch of the corrected workflow instead.

If publication itself fails, inspect the logs and any partial draft or registry uploads before
deciding how to recover. Do not blindly rerun publication or move an existing version tag.

## Try the artifacts on a test machine

On a disposable Linux machine matching the chosen architecture, install Docker 28 or later
(API 1.48+, required for single-platform image export), check out the release source and
enter `nix develop`. With the variables and `downloaded` directory from the examples above, run:

```sh
bash scripts/release-verify.sh "$version" "$arch" downloaded "$image" "$identity"
```

The script verifies the downloads, pulls the image and creates a temporary k3s cluster.
It uses the downloaded CLI to install Zeabur Identity, checks the running server and removes the
cluster when finished. It needs access to GHCR, Sigstore, the k3s image and Nix caches.

For a local build, follow [Building from source](https://github.com/zeabur-oss/identity/blob/main/CONTRIBUTING.md#building-from-source).

Before publishing a new version, follow the [installation checklist](release-check.md)
to check the guide, browser sign-in and account recovery with the release artifacts.
