# Dish — Linux x86_64 preview

Dish is a desktop client for the Pi coding agent. This is pre-release software:
keep backups of projects and review agent actions. The app icon is provisional.

## Requirements

- Linux x86_64 with a working X11 or Wayland desktop and GPU drivers.
- The glibc version shown in `BUILD.json`, or newer. Official candidate builds
  are intended to use Ubuntu 24.04; this is not a universal static Linux binary.
- Runtime libraries: X11/XCB and xkbcommon are required. Wayland, EGL and
  Vulkan libraries are needed for those display stacks. The Debian package
  declares them (`Depends`/`Recommends`); with the archive, install the
  equivalents reported by `ldd bin/dish`, e.g. `libxcb1`, `libxkbcommon0`,
  `libxkbcommon-x11-0`, plus `libwayland-client0`, `libwayland-egl1`,
  `libegl1` and `libvulkan1` when using Wayland or Vulkan. System fonts are
  read from the standard font directories; no separate fontconfig package is
  required.
- Pi installed separately, or configured through Dish's first-run setup.
- Python 3 is needed only by the portable archive installer; the Debian package
  and Dish itself do not require it.

## Install

### Debian/Ubuntu package (recommended)

Download the `.deb` and its `.sha256` sidecar, verify, then let apt resolve the
runtime libraries and register the menu entry:

```sh
sha256sum -c dish-pi_VERSION_amd64.deb.sha256
sudo apt install ./dish-pi_VERSION_amd64.deb
```

Replace `VERSION` with the release version (`0.1.0-alpha.1` in the file name;
the package's own `Version` uses Debian ordering, `0.1.0~alpha.1`).
The package installs the executable to `/usr/bin/dish-pi`, the menu entry and
icon system-wide, and licenses under `/usr/share/doc/dish-pi`. Remove it with
`sudo apt remove dish-pi`; preferences, Pi, projects and conversations are kept.
The package name is `dish-pi` to avoid the collision described below.

### The Debian `dish` package

Debian already ships a package named `dish` (diligence/distributed shell) whose
versions sort higher than ours. Until `0.1.0-alpha.3` our `.deb` used the same
name and wrote to `/usr/bin/dish`, so an `apt full-upgrade` could replace the
Pi client with it. From `0.1.0-alpha.4` the package is called `dish-pi`:

- it installs `/usr/bin/dish-pi`, `/usr/share/applications/dish-pi.desktop`,
  the `dish-pi` icon and its documentation under `/usr/share/doc/dish-pi`;
- it declares no `Conflicts:`, `Replaces:`, `Breaks:` or `Provides: dish`, so
  the legitimate Debian package stays installed and untouched next to it;
- the menu entry is still named **Dish** and matches the window through
  `StartupWMClass=dish`; only the package and the installed paths use `dish-pi`;
- user data (`~/.config/dish`, the Pi install, projects and sessions) is not
  touched.

To migrate an older install, first check which `dish` is present:

```sh
dpkg-query -W -f='${Package} ${Version}\n' dish 2>/dev/null
```

- No output: no `dish` package; install `dish-pi` normally.
- Version `0.1.0~alpha.*`: it is our old `.deb`. Remove it with
  `sudo apt remove dish`, then install `dish-pi`.
- Version `1.19.*` or similar: it is Debian's package. **Do not remove it**;
  install `dish-pi` alongside.

`sudo apt-mark hold dish` prevents the replacement if you want to postpone the
switch, but it also freezes the legitimate Debian package; moving to `dish-pi`
is the definitive fix.

### Arch Linux (AUR)

Two AUR packages are maintained in this repository under `packaging/aur/`:

- **`dish-pi-bin` (recommended)** installs the checksum-pinned CI archive. It
  needs no Rust toolchain and uses the glibc reference build.
- **`dish-pi`** builds the tagged source with `cargo build --release --locked`.
  It needs `rust`, `pkgconf` and the usual GPUI build dependencies.

Both install `/usr/bin/dish-pi`, `/usr/share/applications/dish-pi.desktop`, the
hicolor icon, the license bundle under `/usr/share/licenses/dish-pi`, and
`BUILD.json` under `/usr/share/doc/dish-pi`. The package names use `dish-pi`
because the AUR name `dish` belongs to an unrelated project.

Install the binary package with an AUR helper, or manually:

```sh
# while the AUR mirror is not yet published, build from this repository
cd packaging/aur/dish-pi-bin
makepkg -si
```

Once the mirror exists, `git clone https://aur.archlinux.org/dish-pi-bin.git`
replaces the `cd` line. `dish-pi-bin` declares `provides=('dish-pi')` and
`conflicts=('dish-pi')`, and `dish-pi` declares `conflicts=('dish-pi-bin')`, so
the two variants replace each other instead of installing side by side.

`depends` follows the binary's real `NEEDED` entries: `glibc`, `libgcc`,
`libxcb`, `libxkbcommon` and `libxkbcommon-x11`. Wayland, EGL, Vulkan and the
Vulkan drivers are opened at run time, so they are `optdepends`; the Debian
package recommends the same set.

Use `dish-pi` instead if you want to compile locally. Remove either package with
`sudo pacman -Rns dish-pi-bin` (or `dish-pi`); preferences, Pi, projects and
conversations are kept. `pacman -Ql dish-pi-bin` lists every installed file; it
must show the binary, the desktop entry, the icon, the license bundle and
`BUILD.json`.

The AUR is not updated by CI. For each release, update `_release`, `pkgver`
(Arch versions cannot contain `-`, so `0.1.0-alpha.5` becomes `0.1.0.alpha.5`)
and the two `sha256sums` in both PKGBUILDs, then regenerate their `.SRCINFO`
with `makepkg --printsrcinfo` and push `PKGBUILD` plus `.SRCINFO` to each AUR
repository. The repository files are the review copy; the AUR mirror is manual.
`tests/test_aur_package.py` fails when `ci.yml` and the PKGBUILDs disagree.

### Portable archive (no sudo)

Download the `.tar.gz` and corresponding `.sha256` file into the same directory.
Verify before extracting:

```sh
sha256sum -c dish-VERSION-linux-x86_64.tar.gz.sha256
```

Replace `VERSION` with the actual release version, then extract the archive:

```sh
tar -xzf dish-VERSION-linux-x86_64.tar.gz
cd dish-VERSION-linux-x86_64
./install.sh
```

In a terminal, `./install.sh` shows a short menu — install, uninstall or quit —
and installs when you press Enter. Without a terminal it installs directly, as
before; `./install.sh uninstall` removes the installation non-interactively.

The installer runs without sudo. It writes the executable to `~/.local/bin/dish`,
the desktop entry and icon under `$XDG_DATA_HOME` (default `~/.local/share`), and
third-party licenses under the same data directory. It replaces an existing Dish
installation at those paths. It does not download Pi or run its installer.

### Running Dish

Open Dish through your application menu, or run `dish-pi` (Debian package or
AUR) or `dish` / `~/.local/bin/dish` (portable archive). If Pi is missing,
follow the first-run setup. Set `DISH_PI_BIN` to select a custom Pi executable when
launching from a terminal. Projects and conversations can modify files; this is
not a sandbox.

You may also run `./bin/dish /path/to/project` directly without installing.
GNOME users can set `DISH_BACKEND=x11` to use XWayland and native window borders.

## Uninstall

With the Debian package: `sudo apt remove dish-pi`. With an AUR package:
`sudo pacman -Rns dish-pi-bin` (or `dish-pi`).

With the portable archive: run `./install.sh uninstall` (or `./uninstall.sh`)
from the extracted directory. It removes the installed Dish executable,
shortcut, icon and license files. Preferences, Pi installation, projects and
conversation files are preserved.

## Licenses and source

Dish is Apache-2.0. Dish is based in part on the work of the FreeType Team;
portions are copyright © 1996-2023 The FreeType Project (https://www.freetype.org).
All rights reserved. The selected native FreeType license is FTL.

The archive includes Dish's license, third-party notices and
collected dependency license texts under `share/licenses`. The complete,
unmodified `option-ext` source archive is included there under `sources` for
its MPL-2.0 source-availability requirement. Additional source archives preserve
notices for dependencies whose published packages omitted standalone license
files. Cargo `.crate` files are standard gzip-compressed tar archives.

`BUILD.json` identifies the build target, required glibc version and lockfile
hash. SHA-256 checksums detect corruption; they are not a publisher signature.
