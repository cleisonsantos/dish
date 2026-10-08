# Release license inputs

`overrides.json` supplies license texts omitted by published crate archives.
Most overrides are original files fetched from the repository commit recorded
in the crate's `.cargo_vcs_info.json`, not from a moving branch. Each file is
recorded with its source URL and SHA-256. The package builder verifies the hashes.

For four crates whose original standalone license text could not be retrieved,
the mapping selects a license explicitly declared by the crate and includes the
complete, checksum-verified published source archive in the release, preserving
its authorship and source notices. The standard MIT permission text is kept in
`standard/MIT.txt`; its copyright notices are those in each included source
archive, not a new copyright claim. The CC0 text is the standard CC0-1.0 text.

These files are legal inputs, not development leftovers. They must be reviewed
when the lockfile changes. The builder fails if a crate has no bundled license
file and no matching name/version override.

The builder also includes discovered nested LICENSE/COPYING/COPYRIGHT/NOTICE
files from dependencies, and the original `option-ext` crate archive to satisfy
its MPL-2.0 source-availability requirement. Collection is conservative and may
include build-only dependencies and optional-license alternatives. Do not infer
that all license alternatives apply simultaneously.

`native.json` records additional files that are not named LICENSE/COPYING/NOTICE
but are required by bundled native components. For FreeType, the selected license
is FTL; the file list preserves FTL and contributed-module notices and the crate
source archive is included. Hashes make version/input changes explicit.

Automated collection does not replace reviewing obligations for native code,
data and future dependency changes before publishing an official binary.
