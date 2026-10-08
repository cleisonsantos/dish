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
sha256sum -c dish_VERSION_amd64.deb.sha256
sudo apt install ./dish_VERSION_amd64.deb
```

Replace `VERSION` with the actual package version (`0.1.0~alpha.1` ordering).
The package installs the executable to `/usr/bin/dish`, the menu entry and icon
system-wide, and licenses under `/usr/share/doc/dish`. Remove it with
`sudo apt remove dish`; preferences, Pi, projects and conversations are kept.

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

The installer runs without sudo. It writes the executable to `~/.local/bin/dish`,
the desktop entry and icon under `$XDG_DATA_HOME` (default `~/.local/share`), and
third-party licenses under the same data directory. It replaces an existing Dish
installation at those paths. It does not download Pi or run its installer.

### Running Dish

Open Dish through your application menu, or run `dish` / `~/.local/bin/dish`. If
Pi is missing, follow the first-run setup. Set `DISH_PI_BIN` to select a custom
Pi executable when launching from a terminal. Projects and conversations can
modify files; this is not a sandbox.

You may also run `./bin/dish /path/to/project` directly without installing.
GNOME users can set `DISH_BACKEND=x11` to use XWayland and native window borders.

## Uninstall

With the Debian package: `sudo apt remove dish`.

With the portable archive: run `./uninstall.sh` from the extracted directory. It
removes the installed Dish executable, shortcut, icon and license files. Preferences, Pi installation,
projects and conversation files are preserved.

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
