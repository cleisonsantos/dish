# Preparing a Linux candidate

For the reviewed delivery scope and remaining publication gates for the next
prerelease, see [the alpha.3 readiness assessment](release-alpha3-readiness.md).

Publication is intentionally separate from the build scripts. None of these
commands creates a GitHub repository, tag or Release, or uploads a package.

## Local checks

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
```

Use Python 3.11+ and Linux x86_64. Packaging additionally requires Cargo/Rust,
`readelf`, `strings` (binutils), the Rust `rust-docs` component for standard-library
notices, the Linux build dependencies from the README, and HTTPS access to fetch the Rust toolchain's license files at its exact commit.

```sh
python3 scripts/package-linux.py --version 0.1.0-alpha.1
```

The version label must match the base version in Cargo.toml. The script:

1. Builds the explicit Linux x86_64 target, with Rust path remapping and stripped
   symbols. Existing compiler flags are preserved.
2. Rejects a non-x86_64 ELF or a binary containing personal home paths.
3. Records the required glibc version. A successful local build does not prove
   compatibility with older Linux distributions.
4. Collects dependency licenses and native notices, checks pinned overrides,
   and includes checksum-verified original source archives where required.
5. Adds the exact Rust toolchain's license texts, copyright notices and the
   installed standard-library third-party license bundle.
6. Packages the executable, provisional icon, installer/uninstaller, documentation
   and build metadata, then writes a SHA-256 sidecar under ignored `dist/`.

```sh
python3 scripts/package-deb.py --version 0.1.0-alpha.1
```

`package-deb.py` converts that verified archive into `dish-pi_0.1.0-alpha.1_amd64.deb`
(plus a SHA-256 sidecar). It reuses the archive's payload and license bundle
unchanged, derives `Depends:` from the binary's real `NEEDED` entries and the
runner's package database, and declares dlopen-only libraries (EGL, Vulkan,
Wayland) as `Recommends:`. The Debian `Version` maps prereleases to `~` ordering
(`0.1.0-alpha.1` → `0.1.0~alpha.1`); the file name keeps the release spelling
because GitHub normalizes `~` in asset names. `dpkg-deb` and `dpkg-query` are
required; both are present on Debian/Ubuntu.

`--skip-build` reuses the target-specific binary but still runs privacy and
license checks. Use this only after a successful remapped build; it is not a
way to bypass release checks. The archive is a candidate, not an automatic legal
or compatibility certification. Archives are not promised to be byte-for-byte
reproducible (timestamps and compiler versions can differ).

## Test the actual archive

```sh
export DISH_TEST_ARCHIVE="$PWD/dist/dish-0.1.0-alpha.1-linux-x86_64.tar.gz"
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
```

The archive test verifies the checksum, collected license files and source
availability, installs into isolated temporary user directories, and uninstalls.
It never installs into the real user's home directory.

Extract a verified archive to a temporary directory and set `DISH_TEST_BIN` to
its `bin/dish`. This overrides the default executable used by the UI tests:

```sh
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/desktop_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/startup_smoke.py
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/settings_smoke.py
```

The same variable can point at the system-wide executable from an installed
`.deb` (`DISH_TEST_BIN=/usr/bin/dish-pi`) to smoke-test the installed package.

These use a synthetic Pi and isolated configuration. Install the Xvfb, XTest,
xclip, Tesseract and ImageMagick dependencies described in the README. Headless
hosts also need Mesa/EGL software graphics (`libegl1`, `libgl1-mesa-dri`,
`mesa-vulkan-drivers`); CI sets `LIBGL_ALWAYS_SOFTWARE=1` and creates a private
`XDG_RUNTIME_DIR` for its virtual displays.

## Windows installer

```sh
python3 scripts/package-windows.py            # requires cargo build --release first
python3 scripts/package-windows.py --bundle-only --target x86_64-unknown-linux-gnu
```

The script stages `dist/windows-payload/` (executable, `LICENSE`,
`THIRD_PARTY_NOTICES.md`, `BUILD.json` and the shared license bundle) and then
runs NSIS to produce `dish-<version>-windows-x86_64-setup.exe` plus its SHA-256
sidecar. `--bundle-only` is for checking the license collection without NSIS.
The version label comes from `ci.yml` (single source), with `Cargo.toml` as the
fallback, and `makensis` is looked up on `PATH` and in the common Chocolatey and
NSIS install directories.

The installer is per-user (no administrator), adds Start Menu and optional
desktop shortcuts, registers an uninstaller in Add/Remove Programs, and offers
the official Pi installer when `where pi` finds nothing. Uninstalling removes
only Dish.

**Licenses are not optional.** The Windows payload must carry the same bundle as
the Linux archive (issue #63): `scripts/license_bundle.py` collects the
dependency texts, the hash-pinned overrides and native notices, the Rust
toolchain notices and the MPL source archive. A missing license entry stops the
build, on every platform. macOS packaging must use the same module.

When a GitHub Release is published, `.github/workflows/windows.yml` rebuilds the
installer in that run and attaches it plus its `.sha256` sidecar through
`gh release upload`. Pushing a branch or a tag publishes nothing; it only saves
short-lived Actions artifacts. The publish job refuses an installer whose
version does not match the release tag, verifies the checksum sidecar, and never
overwrites an existing asset (`--clobber` is deliberately absent), so a wrong or
replaced build fails closed. Publishing Windows assets is therefore a human
decision, made by creating the Release.

### Windows code signing (SignPath Foundation)

The [code signing policy](code-signing.md) names
[@cleisonsantos](https://github.com/cleisonsantos) as the approver. This maintainer
reviews and manually approves each signing request in the SignPath dashboard
for every published release; approval is not automatic or carried over from a
previous release.

Configure these repository **Variables** under Settings > Secrets and variables
> Actions > Variables:

- `SIGNPATH_ORGANIZATION_ID`
- `SIGNPATH_PROJECT_SLUG`
- `SIGNPATH_SIGNING_POLICY_SLUG`
- `SIGNPATH_ARTIFACT_CONFIGURATION_SLUG`

Store `SIGNPATH_API_TOKEN` as a repository **Secret**, never as a variable or in
source. The SignPath project, signing policy and artifact configuration must be
provisioned in the dashboard to accept this repository's raw `dish.exe`, NSIS
`.exe` and `.msi` requests. They are not created by the workflow.

Only `release: published` runs with a nonempty `SIGNPATH_PROJECT_SLUG` submit
requests. All jobs through signing run on GitHub-hosted runners. Push, PR and
manual builds never contact SignPath. With SignPath enabled, a missing token or
required variable fails the job before building; rejected requests, timeouts,
missing signed output and invalid Authenticode signatures also fail closed.

There are **three sequential manual approvals per release**: first `dish.exe`,
which is restored and verified with `signtool verify /pa /v` before staging the
shared NSIS/MSI payload; then the NSIS installer and MSI package after both have
been built. The approver checks the repository, release commit, artifact and
policy in the dashboard before approving each request. Keep the dashboard open
while the job waits: all approvals and builds must fit the workflow's 60-minute
job timeout. The workflow verifies each returned file and computes final
installer checksums only after signing, before uploading release assets.

Without `SIGNPATH_PROJECT_SLUG`, SignPath is disabled and the default release is
**unsigned**. Release notes **must explicitly state that Windows artifacts are
unsigned**; do not claim compliance with the signing policy for that release.
The pre-existing PFX opt-in path (`WINDOWS_CERT_PFX_BASE64` and
`WINDOWS_CERT_PASSWORD`) remains a separate legacy fallback when SignPath is
disabled; it is not SignPath Foundation signing. If that fallback is explicitly
used, verify the final signatures and describe their actual issuer in the
release notes instead. PFX steps are skipped for SignPath-enabled releases so
they cannot overwrite the Foundation signatures.

## GitHub CI

`.github/workflows/ci.yml` builds on Ubuntu 24.04, runs strict Clippy and unit
checks, produces a candidate plus a Debian package, installs the package with
`apt` and smoke-tests the installed `/usr/bin/dish-pi` (the Debian package is
named `dish-pi` so it never disputes the `dish` package Debian ships), then
saves both as short-lived Actions artifacts. It has read-only
repository permissions and no release-publication step. The configured alpha
version label must be updated when Cargo.toml's base version changes.

`.github/workflows/windows.yml` does the same for Windows: strict Clippy, a
release build, a GUI-subsystem check on `dish.exe`, and the NSIS installer with
the same license bundle. Both workflows keep `contents: read` as the default;
only the Windows `publish` job, gated on `release: published`, is granted
`contents: write` to attach the installer to that Release.

Only use a tested CI-built archive for an official release, not a locally built
binary with a newer glibc requirement. Inspect its `BUILD.json`, dynamic library
requirements, privacy scan and license bundle. Test on a real Ubuntu 24.04
desktop (Wayland and X11) before claiming support beyond the local Xvfb tests.

## Publishing a complete release

The Linux assets are produced by `ci.yml` on Ubuntu 24.04 but are not attached
by that workflow. Publish them in this order from a clean checkout:

1. Wait for green CI, then download the verified candidate that the maintainer
   will attach:

   ```sh
   gh run download RUN_ID -n dish-linux-x86_64-candidate -D /tmp/dish-release-assets
   ```

2. Create the Release as a draft. Keep it a prerelease when the version has a
   prerelease suffix:

   ```sh
   gh release create v0.1.0-alpha.6 --draft --prerelease \
     --title "Dish v0.1.0-alpha.6" --notes-file /tmp/dish-release-notes.md
   ```

3. Run the publisher on that directory:

   ```sh
   scripts/publish-release.sh v0.1.0-alpha.6 /tmp/dish-release-assets
   ```

   The secret key comes from `MINISIGN_SECRET_KEY`, or from
   `--secret-key-file PATH` when it lives on disk. The script checks its inputs
   before touching GitHub: the tag and the four files, both local SHA-256
   sidecars, the required tools, and a key pair proof against the public key in
   `src/update.rs` (`--public-key PATH` overrides it for tests). Without a key,
   or when the private key does not match `UPDATE_PUBLIC_KEY`, it exits before
   any GitHub call. It also runs an unsigned trial of the manifest generator
   over the local files (`BUILD.json`, version, target, glibc and checksums), so
   an asset the generator refuses never reaches the point of publishing the
   draft.

   It then uploads the two Linux packages and their `.sha256` files without
   `--clobber`. An asset already present on the Release must have the same size
   and, when GitHub reports a digest, the same SHA-256; otherwise it aborts. If
   the Release is still a draft, it publishes it and re-reads the final
   `publishedAt`, because draft download URLs are `untagged-...` and cannot be
   used in the manifest.

   The manifest is generated only after the Release is public and its assets
   can be downloaded: the script downloads the four Linux files again, checks
   their sidecars, generates and signs `latest.json` over those exact bytes, and
   runs `package-manifest.py --check-release` so that every asset name, size,
   URL and digest matches the Release. Only then does it upload `latest.json`
   and `latest.json.minisig`, as the last two assets. Finally it downloads both
   files into a fresh directory, compares them with the signed copies, verifies
   the signature again and repeats the release check. It refuses to start when
   either manifest asset already exists, and never replaces an asset.

4. Publishing the draft also starts the `publish` job in
   `.github/workflows/windows.yml`, which builds and attaches the Windows
   `.exe`/`.msi` and their `.sha256` files. Those Windows artifacts are not part
   of `latest.json`; the Linux assets are the only update payload.

The manifest is not generated inside a workflow. The Linux assets are
attached by hand outside the workflow, and the Windows `publish` job runs on the
publication event with no guarantee that the Linux assets exist yet. Draft URLs
also contain `untagged-...`, so the final asset URLs and `published_at` are only
known after publication. Generating earlier would produce a manifest that the
client rejects.

There is a short window between publishing the draft and attaching the manifest.
During it the client finds the Release but no `latest.json`, so its update check
fails and it tries again on a later check; it shows no incorrect update. The
release is complete only when the script prints the final confirmation and exits
successfully. If it fails after publication, remove `latest.json` and
`latest.json.minisig` from the Release by hand and run it again; the script says
so explicitly and will not overwrite the partial result.

## Before publication

- Review the final staged files and scan the resulting Git history with Gitleaks.
- Confirm commit identity, the provisional icon and known limitations.
- Review third-party notices and dependency/native license inputs for the exact
  lockfile. A new dependency with missing license text makes packaging fail.
- Scan the extracted candidate, including source archives, with redacted output.
- Require the CI checks to pass and verify downloaded artifact checksums.
- Publish `v0.1.0-alpha.1` explicitly as a prerelease with
  `scripts/publish-release.sh`, including compatibility details and known
  limitations. The script refuses to run without the release key and only
  finishes after `latest.json` matches the published assets. No stable-release
  promise is implied by this candidate.
