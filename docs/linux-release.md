# Dish — Linux x86_64 beta

Dish is a desktop client for the Pi coding agent. This is beta software:
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

## Linux formats

- Portable `.tar.gz`: use on Arch or other glibc distributions for a per-user install without sudo.
- `.deb`: use on Debian and Ubuntu for package-manager installation.
- `.rpm`: use on RPM-based distributions for package-manager installation.
- `.AppImage`: the quickest way to try Dish without installing; running it requires FUSE.

All formats use the system runtime libraries and the glibc minimum in `BUILD.json`;
the AppImage does not bundle those libraries. Verify each download using its `.sha256`.

## Install

### Install without sudo (one command)

On Linux x86_64, the bootstrap installer downloads the portable archive from
GitHub Releases, verifies its SHA-256 and runs the archive's own `install.sh`, so
the result is the same as the portable archive section below:

```sh
curl -fsSL https://raw.githubusercontent.com/cleisonsantos/dish/main/scripts/install.sh | sh
```

Install a specific version by passing the tag (with or without the leading `v`):

```sh
curl -fsSL https://raw.githubusercontent.com/cleisonsantos/dish/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
```

The installer refuses to run as root, because it writes only to your home
directory and never needs sudo. Without `--version` it resolves the newest
release from the GitHub API, including prereleases. It checks the downloaded
archive against the `.sha256` from the same release before extracting anything,
then extracts to a temporary directory and delegates to the packaged installer.

Options:

- `--version <tag>`: install a specific release (`v0.1.0-beta.1` or `0.1.0-beta.1`).
- `--prefix <dir>`: install under `<dir>` instead of `$HOME`. The packaged
  installer runs with `HOME=<dir>` and `XDG_DATA_HOME=<dir>/.local/share`, so
  the executable goes to `<dir>/.local/bin/dish` and the desktop entry, icon and
  licenses to `<dir>/.local/share`.
- `--no-desktop`: install the program without leaving an application-menu entry.
- `-y`, `--yes`: do not ask for confirmation.
- `--uninstall`: remove the installed files.

Verification is checksum-only for now. The checksum comes from the same release,
so it protects against corrupted or truncated downloads, not against a
compromised release. Minisign signature verification will be added once signed
manifests are published. The `.deb`, `.rpm`, AppImage and manual `.tar.gz`
downloads remain available.

### RPM

Verify the `.rpm` and its checksum with `sha256sum -c`, then install it with
`sudo dnf install ./dish-VERSION-1.x86_64.rpm`. The package is named `dish-pi`
and installs `dish-pi` alongside any existing `dish` program. Remove it with
`sudo dnf remove dish-pi`.

### AppImage

Verify the `.AppImage` with `sha256sum -c`, make it executable with `chmod +x`,
and run it directly. It requires FUSE at runtime, along with the system libraries
listed below. The build tool itself uses `--appimage-extract-and-run` in CI because
the runner has no FUSE. Remove the AppImage file to uninstall it.

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

Open Dish through your application menu, or run `dish-pi` (Debian package) or
`dish` / `~/.local/bin/dish` (portable archive), or run the AppImage directly. If Pi is missing, follow the
first-run setup. Set `DISH_PI_BIN` to select a custom Pi executable when
launching from a terminal. Projects and conversations can modify files; this is
not a sandbox.

You may also run `./bin/dish /path/to/project` directly without installing.
GNOME users can set `DISH_BACKEND=x11` to use XWayland and native window borders.

## Uninstall

With the Debian package: `sudo apt remove dish-pi`.

With the one-command installer, run `curl -fsSL https://raw.githubusercontent.com/cleisonsantos/dish/main/scripts/install.sh | sh -s -- --uninstall`.

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
