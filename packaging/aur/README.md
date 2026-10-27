# Arch packages (AUR)

This directory holds the review copies of the two AUR packages:

| Package | Source | Use it when |
| --- | --- | --- |
| `dish-pi-bin` | GitHub Release `.tar.gz` | Normal installation. No Rust toolchain; uses the CI glibc 2.39 reference build. |
| `dish-pi` | GitHub tag source archive | You prefer a local build with `cargo build --release --locked`. It needs the full GPUI toolchain. |

The package name is `dish-pi`, not `dish`: the AUR name `dish` already belongs
to an unrelated project. Both packages install the same paths, so they conflict
with each other. `dish-pi-bin` also declares `provides=('dish-pi')`, allowing a
package manager to replace one variant with the other.

## Test locally

`makepkg` and the runtime dependencies are needed. From the package directory:

```sh
cd packaging/aur/dish-pi-bin
makepkg -si
```

To inspect without installing, build only:

```sh
makepkg            # só empacota, não instala
pacman -Qip dish-pi-bin-*.pkg.tar.zst
bsdtar -tf dish-pi-bin-*.pkg.tar.zst | sort
```

The expected installed files are:

- `/usr/bin/dish-pi`
- `/usr/share/applications/dish-pi.desktop`
- `/usr/share/icons/hicolor/512x512/apps/dish-pi.png`
- `/usr/share/licenses/dish-pi/` (the complete license bundle)
- `/usr/share/doc/dish-pi/BUILD.json` and `README.md`

`dish-pi-bin` copies the license bundle shipped inside the release archive
(every dependency text plus the Rust toolchain notices). `dish-pi` builds from
the tag, which only contains the license *inputs* (`licenses/standard`,
`overrides.json`, `native.json`) — it installs `LICENSE`,
`THIRD_PARTY_NOTICES.md` and those inputs, not the generated per-crate bundle.

Remove it with `sudo pacman -Rns dish-pi-bin`. `pacman -Ql dish-pi-bin` lists
everything the package owns; preferences, Pi, projects and sessions are kept.

For the source package, replace `dish-pi-bin` with `dish-pi`. A full build takes
about two minutes on a modern machine and needs `rust`, `pkgconf`, `fontconfig`,
`freetype2` and `wayland` in addition to the runtime libraries. Its `prepare()`
runs `cargo fetch --locked`, so the network is used before `build()` starts.

If the dependencies are already installed and `makepkg` still refuses to resolve
`rust` (for example with a toolchain installed by rustup instead of the `rust`
package), `makepkg --nodeps -f` builds against the local toolchain. That is a
diagnostic shortcut, never a publishing path.

## Updating a release

`pkgver` cannot contain a hyphen, so the GitHub release `0.1.0-alpha.5` is
`0.1.0.alpha.5` in the PKGBUILD. The source and asset names keep the release
spelling in `_release`.

1. Set `_release` to the new release label in both PKGBUILDs.
2. Set `pkgver` to the same label with `-` replaced by `.`.
3. Replace the binary archive checksum with the value in the release's
   `dish-<version>-linux-x86_64.tar.gz.sha256` sidecar.
4. Replace the source archive checksum with
   `sha256sum` of `https://github.com/cleisonsantos/dish/archive/refs/tags/v<version>.tar.gz`.
5. Regenerate the metadata and run the tests:

   ```sh
   (cd packaging/aur/dish-pi-bin && makepkg --printsrcinfo > .SRCINFO)
   (cd packaging/aur/dish-pi && makepkg --printsrcinfo > .SRCINFO)
   python3 -m unittest discover -s tests -p 'test_aur_package.py' -v
   ```

6. Verify both sources without compiling: `makepkg --verifysource` in each
   directory. Build at least the binary package with `makepkg --nodeps`.

The version label is the single source of truth in `.github/workflows/ci.yml`;
`tests/test_aur_package.py` fails when `_release`, `pkgver`, the `.SRCINFO` or
the `Cargo.toml` base version disagree.

## Mirror to the AUR

The AUR is a separate git repository per package, over SSH; CI does not publish
there. After review, copy `PKGBUILD` and `.SRCINFO` into a clone of
`ssh://aur@aur.archlinux.org/dish-pi-bin.git` and
`ssh://aur@aur.archlinux.org/dish-pi.git`, commit and push each one manually.
This step is intentionally manual: it requires the maintainer's SSH key and
should happen only after the release artifacts exist and their checksums match.
