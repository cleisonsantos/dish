# Preparing a Linux candidate

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

`package-deb.py` converts that verified archive into `dish_0.1.0~alpha.1_amd64.deb`
(plus a SHA-256 sidecar). It reuses the archive's payload and license bundle
unchanged, derives `Depends:` from the binary's real `NEEDED` entries and the
runner's package database, and declares dlopen-only libraries (EGL, Vulkan,
Wayland) as `Recommends:`. The Debian version maps prereleases to `~` ordering
(`0.1.0-alpha.1` → `0.1.0~alpha.1`). `dpkg-deb` and `dpkg-query` are required;
both are present on Debian/Ubuntu.

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
`.deb` (`DISH_TEST_BIN=/usr/bin/dish`) to smoke-test the installed package.

These use a synthetic Pi and isolated configuration. Install the Xvfb, XTest,
xclip, Tesseract and ImageMagick dependencies described in the README. Headless
hosts also need Mesa/EGL software graphics (`libegl1`, `libgl1-mesa-dri`,
`mesa-vulkan-drivers`); CI sets `LIBGL_ALWAYS_SOFTWARE=1` and creates a private
`XDG_RUNTIME_DIR` for its virtual displays.

## GitHub CI

`.github/workflows/ci.yml` builds on Ubuntu 24.04, runs strict Clippy and unit
checks, produces a candidate plus a Debian package, installs the package with
`apt` and smoke-tests the installed `/usr/bin/dish`, then saves both as
short-lived Actions artifacts. It has read-only
repository permissions and no release-publication step. The configured alpha
version label must be updated when Cargo.toml's base version changes.

Only use a tested CI-built archive for an official release, not a locally built
binary with a newer glibc requirement. Inspect its `BUILD.json`, dynamic library
requirements, privacy scan and license bundle. Test on a real Ubuntu 24.04
desktop (Wayland and X11) before claiming support beyond the local Xvfb tests.

## Before publication

- Review the final staged files and scan the resulting Git history with Gitleaks.
- Confirm commit identity, the provisional icon and known limitations.
- Review third-party notices and dependency/native license inputs for the exact
  lockfile. A new dependency with missing license text makes packaging fail.
- Scan the extracted candidate, including source archives, with redacted output.
- Require the CI checks to pass and verify downloaded artifact checksums.
- Publish `v0.1.0-alpha.1` explicitly as a prerelease, with compatibility details
  and known limitations. No stable-release promise is implied by this candidate.
