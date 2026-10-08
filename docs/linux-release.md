# Dish — Linux x86_64 preview

Dish is a desktop client for the Pi coding agent. This is pre-release software:
keep backups of projects and review agent actions. The app icon is provisional.

## Requirements

- Linux x86_64 with a working X11 or Wayland desktop and GPU drivers.
- The glibc version shown in `BUILD.json`, or newer. Official candidate builds
  are intended to use Ubuntu 24.04; this is not a universal static Linux binary.
- Runtime libraries for fontconfig, FreeType, X11/XCB, Wayland and xkbcommon.
  On Ubuntu, the usual packages are `libfontconfig1`, `libfreetype6`,
  `libx11-xcb1`, `libxcb1`, `libwayland-client0`, `libwayland-cursor0`,
  `libwayland-egl1`, `libxkbcommon0` and `libxkbcommon-x11-0`, plus the
  desktop's graphics/Vulkan drivers. Additional system dependencies can vary
  with the exact build; inspect missing libraries with `ldd bin/dish`.
- Python 3 for installing the desktop shortcut; it is not required to run Dish.
- Pi installed separately, or configured through Dish's first-run setup.

## Verify and install

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

Open Dish through your application menu, or run `~/.local/bin/dish`. If Pi is
missing, follow the first-run setup. Set `DISH_PI_BIN` to select a custom Pi
executable when launching from a terminal. Projects and conversations can modify
files; this is not a sandbox.

You may also run `./bin/dish /path/to/project` directly without installing.
GNOME users can set `DISH_BACKEND=x11` to use XWayland and native window borders.

## Uninstall

Run `./uninstall.sh` from the extracted archive. It removes the installed Dish
executable, shortcut, icon and license files. Preferences, Pi installation,
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
