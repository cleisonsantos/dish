# Third-party notices

Dish is licensed under Apache-2.0; see `LICENSE`.

## Adapted source

`src/editor.rs` contains code adapted from the `Editor` in the gpui-ce
`view_example`:

- Upstream: https://github.com/gpui-ce/gpui-ce
- License: Apache-2.0
- Upstream copyright: Copyright 2022 - 2025 Zed Industries, Inc.
- Verified against the published gpui-ce 0.2.2 `LICENSE-APACHE`.
- Upstream license: https://github.com/gpui-ce/gpui-ce/blob/main/LICENSE-APACHE
- Dish modifications include wrapping, cursor movement, selection, clipboard
  handling and undo/redo. See the source file for implementation details.

## Dependencies

Cargo dependencies retain their respective licenses. `Cargo.lock` records the
versions used by the application. The Apache-2.0 license for Dish does not
relicense its dependencies.

### Requirements for binary releases

A binary release must include this document, the Dish `LICENSE`, and the
applicable dependency license texts and attribution notices. This file alone
is not a complete dependency license bundle. Generate and review that bundle
from the exact `Cargo.lock`, target and features used to build the release.
Native code bundled by Rust crates also needs review; crate SPDX metadata alone
is not sufficient to establish every distribution requirement.

Notable dependencies in the initial Linux dependency graph:

- `option-ext` 0.2.0 is licensed under MPL-2.0. Its license and source-availability
  notice must accompany the binary distribution. Source for that exact version
  is available at https://crates.io/api/v1/crates/option-ext/0.2.0/download .
  MPL-2.0 does not require relicensing the separate Dish source files under MPL.
- `self_cell` 1.3.0 offers `Apache-2.0 OR GPL-2.0-only`; use the Apache-2.0
  option for the release, preserving its applicable notices.
- Unicode-3.0 dependencies require preserving their copyright and permission
  notices; `unicode-ident` additionally has an AND requirement for Unicode-3.0
  alongside its MIT/Apache-2.0 choice.

### FreeType native library

Dish is based in part on the work of the FreeType Team. Portions of this software
are copyright © 1996-2023 The FreeType Project (https://www.freetype.org).
All rights reserved.

The native FreeType 2.13.2 sources bundled by `freetype-sys` 0.20.1 are used under
the FreeType License (FTL), not its alternative GPL license. The Rust wrapper's
MIT declaration does not replace the native library's license. The candidate
archive includes `FTL.TXT`, the relevant contributed-module license notices,
and the original checksum-verified crate source archive. See `licenses/native.json`
for the pinned inputs.

Pi is a separate application, not bundled with Dish. Its licenses and terms
apply independently.
