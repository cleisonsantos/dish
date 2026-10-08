# Pre-release privacy and licensing audit

## Scope and method

This audit covers the current Git-eligible working tree (tracked and untracked
files, excluding Git-ignored files), not an already-published release. The
initial snapshot contained 37 files. There is no commit history to scan yet.

Checks performed:

- Enumerated Git-eligible files and inspected included assets and test fixtures.
- Scanned an isolated copy of those files with Gitleaks 8.30.1, default rules,
  redacted output and allow-comments disabled. The official release download
  was checked against its published SHA-256 checksum.
- Ran additional patterns for private keys, common provider tokens, credential
  assignments, credential-bearing URLs, personal home paths and email addresses.
- Inspected PNG chunks for text/EXIF metadata and SVGs for external resources.
- Inspected Cargo metadata with `--locked --offline --filter-platform
  x86_64-unknown-linux-gnu`, and reviewed selected dependency license files.
- Checked the existing local release binary for embedded personal build paths.

The checks are a point-in-time technical review, not a guarantee that all secrets
or legal obligations have been identified. They are not a vulnerability audit
or a formal legal opinion. Gitleaks and the Cargo metadata output were kept
outside the repository.

## Privacy findings

### Source tree: no detected credentials or personal data

Gitleaks reported no leaks. The additional patterns found no matching credentials,
personal home paths or email addresses in the initial Git-eligible snapshot.
There were no symlinks. The only binary source asset was `assets/dish-icon.png`;
it contained image/background chunks, with no text or EXIF chunks. Included SVGs
contained no scripts or external image/font references.

The test fixtures examined use synthetic Pi messages and temporary project and
configuration directories. No real Pi session or authentication file is included.
Generated build outputs are Git-ignored. Previous development material was moved
outside the repository and was not part of the public-tree scan.

### Commit metadata: owner decision needed

With the owner's approval, the repository-local Git email was changed to the
GitHub ID-based noreply address derived from the authenticated account. Global
Git settings were not changed. There is no existing commit history to rewrite.
Do not assume that changing account visibility retroactively changes commits.

### Binary release: do not publish the existing local artifact

The existing `target/release/dish` contained 1,120 matches for personal home-path
prefixes. These can come from embedded source locations and dependency paths;
stripping symbols alone is not a reliable fix.

Build release artifacts in a clean CI environment and apply Rust path remapping
for the project root and dependency/toolchain source roots as appropriate. Scan
the resulting artifact again before uploading. Source-tree cleanliness does not
imply binary-artifact cleanliness.

## Licensing findings

### Project license

The root `LICENSE` contains Apache-2.0 and `Cargo.toml` declares `Apache-2.0`.

### Adapted editor: attribution corrected

The published gpui-ce 0.2.2 `LICENSE-APACHE` includes:

> Copyright 2022 - 2025 Zed Industries, Inc.

That notice was missing from Dish's attribution. It has now been added to
`src/editor.rs` and `THIRD_PARTY_NOTICES.md`. The source already describes
Dish's modifications. No separate upstream NOTICE file was found in the local
published gpui-ce or gpui_ce_platform crate packages inspected.

### Dependency inventory: binary packaging work remains

Cargo metadata listed 511 resolved packages including Dish. No package in that
inventory lacked a declared license. Metadata includes build and procedural-macro
dependencies; this is a conservative inventory, not an exact list of code linked
into the executable.

Notable obligations:

- `option-ext` 0.2.0: MPL-2.0, including license and source-availability obligations
  for the covered dependency. This does not relicense separate Dish files.
- `self_cell` 1.3.0: Apache-2.0 OR GPL-2.0-only. Select the Apache-2.0 option;
  the presence of the GPL alternative does not require selecting it.
- Unicode-3.0 dependencies: preserve their copyright and permission notices.
  For `unicode-ident`, Unicode-3.0 applies in addition to its MIT/Apache choice.
- MIT, BSD, ISC, Zlib and Apache dependencies also have applicable notices and
  license conditions that must be preserved when distributing their code.

Before a binary release, generate and review a dependency license bundle for the
exact lockfile, target and enabled features. Review bundled native code and data,
not just Cargo SPDX fields. Include the resulting license texts and notices in
the release archive. `THIRD_PARTY_NOTICES.md` alone is not that complete bundle.

The current app assets were created during this project's development; no bundled
font files or external icon library were found in the project's asset directory.
Pi is a separate program and is not bundled with Dish.

## Follow-up: local candidate preparation

Strict Clippy now passes without warnings. The candidate packaging tool creates
an archive with license entries for all 510 dependency packages in this lockfile,
28 version-pinned overrides for missing upstream license files, native FreeType
FTL/contributed-module notices, Rust toolchain license texts, and verified source
archives for MPL/source-notice preservation. The FreeType credit was added to
`THIRD_PARTY_NOTICES.md`; its native license is FTL, not the Rust wrapper's MIT
license or FreeType's alternative GPL license.

A target-specific remapped local build passed the binary home-path check. The
local candidate still requires glibc 2.43, so it is for local validation only,
not the official Ubuntu 24.04 artifact. The workflow is configured to build and
test a candidate on Ubuntu 24.04 but has not been run on GitHub yet.

Eight release-tool/archive tests and 34 Rust tests passed. Desktop, settings and
startup UI smoke tests passed against the extracted candidate executable. The
source tree and final package must be rescanned after further changes. Automated
license collection still requires a final review before public distribution.

## Publication gates

- [x] Owner confirms the commit email identity; repository-local noreply configured.
- [ ] Review the final staged file list and rerun a redacted secret scan.
- [ ] After committing, scan the actual history before the first push.
- [x] Generate the dependency/native/toolchain license bundle for the local candidate.
- [ ] Finish the distribution review for the exact CI-built archive.
- [ ] Build the Linux artifact in CI with appropriate path remapping.
- [ ] Inspect and scan the final archive, including installer and metadata.

No push, repository creation or release publication was performed during the
initial audit.

## GitHub publication follow-up

With the owner's approval, the public repository was created at
https://github.com/cleisonsantos/dish and the source was pushed to `main`.
Gitleaks found no leaks in the initial published history. Build outputs under
`target/` and local candidate archives under `dist/` remain Git-ignored.

The first clean CI exposed a missing `libxkbcommon-x11-dev` build dependency;
the workflow and README were corrected. The next run exposed a transient Unix
ETXTBSY executable-start race. A bounded, error-specific retry and regression
test were added; all 35 Rust tests pass locally. Rust build caching was also
added. The next GitHub run must still validate the complete package pipeline.
No GitHub Release or version tag has been published.
