#!/usr/bin/env python3
"""Build a Debian package from a verified Linux candidate archive. Never uploads.

The .deb is built from the exact tar.gz produced by scripts/package-linux.py, so
the payload, license bundle and checksums stay identical. Runtime dependencies
are derived from the binary's real dynamic linkage and the runner's package
database, so a lockfile or dependency change cannot silently drop them.
"""
import argparse
import email.utils
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-gnu"
DEB_ARCH = "amd64"
MAINTAINER = "Cleison Santos <33238294+cleisonsantos@users.noreply.github.com>"
HOMEPAGE = "https://github.com/cleisonsantos/dish"
# Libraries loaded with dlopen at runtime; without at least one graphics stack
# Wayland/Vulkan setups fail to start. They cannot appear as hard dependencies.
RECOMMENDS = ["libegl1", "libvulkan1", "libwayland-client0", "libwayland-egl1"]
BASE_VERSION = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def debian_version(version: str) -> str:
    """Map a release version to Debian ordering: 0.1.0-alpha.1 -> 0.1.0~alpha.1."""
    base, separator, prerelease = version.partition("-")
    if not separator:
        if not BASE_VERSION.match(base):
            raise ValueError(f"Not a x.y.z version: {version}")
        return base
    if not BASE_VERSION.match(base) or not re.fullmatch(r"[A-Za-z0-9]+(?:[.\-][A-Za-z0-9]+)*", prerelease):
        raise ValueError(f"Not a valid prerelease version: {version}")
    return f"{base}~{prerelease.replace('-', '.')}"


def run(*args, **kwargs):
    env = kwargs.pop("env", os.environ.copy())
    env["LC_ALL"] = "C"
    for key in ["LANGUAGE", "LANG"]:
        env.pop(key, None)
    return subprocess.check_output(args, cwd=ROOT, text=True, stderr=subprocess.DEVNULL, env=env, **kwargs)


PACKAGE_NAME = re.compile(r"^[a-z0-9][a-z0-9+.-]*$")


def needed_libraries(binary: Path) -> list[str]:
    """Direct NEEDED entries; transitive libraries are left to their own packages."""
    names = []
    for line in run("readelf", "-d", str(binary)).splitlines():
        if "NEEDED" not in line:
            continue
        name = line.split("[", 1)[-1].rsplit("]", 1)[0].strip()
        if name:
            names.append(name)
    return sorted(set(names))


def linked_packages(binary: Path) -> list[str]:
    resolved = {}
    for line in run("ldd", str(binary)).splitlines():
        if "=>" in line:
            name, path = (part.strip() for part in line.split("=>", 1))
        else:
            # The interpreter (ld-linux) has no "=>" entry.
            path = line.strip().split(" ")[0]
            name = os.path.basename(path) if path.startswith("/") else ""
        path = path.split(" ")[0]
        if name and path.startswith("/"):
            resolved[name] = path
    packages = set()
    for name in needed_libraries(binary):
        path = resolved.get(name)
        if not path:
            raise RuntimeError(f"Cannot resolve shared library path: {name}")
        candidates = [path]
        real = os.path.realpath(path)
        if real != path:
            candidates.append(real)
        found = None
        for candidate in candidates:
            try:
                query = run("dpkg-query", "-S", candidate)
            except subprocess.CalledProcessError:
                continue
            for line in query.splitlines():
                # dpkg-query prints "pkg[:arch]: /path" and can add diversion lines.
                package, separator, _ = line.partition(": ")
                candidate_name = package.split(":")[0]
                if separator and PACKAGE_NAME.match(candidate_name):
                    found = candidate_name
                    break
            if found:
                break
        if not found:
            raise RuntimeError(f"Cannot map shared library to a Debian package: {name} ({path})")
        packages.add(found)
    return sorted(packages)


def control_text(version: str, glibc: str, dependencies: list[str], installed_kb: int = 0) -> str:
    packages = [f"libc6 (>= {glibc})" if name == "libc6" else name for name in dependencies]
    if "libc6" not in dependencies:
        packages.insert(0, f"libc6 (>= {glibc})")
    lines = [
        "Package: dish",
        f"Version: {version}",
        f"Architecture: {DEB_ARCH}",
        f"Maintainer: {MAINTAINER}",
        f"Homepage: {HOMEPAGE}",
        "Section: devel",
        "Priority: optional",
        f"Depends: {', '.join(packages)}",
        f"Recommends: {', '.join(RECOMMENDS)}",
        f"Installed-Size: {installed_kb}",
        "Description: GPU-accelerated desktop interface for the Pi coding agent",
        " Dish is a client for the Pi coding agent: it launches `pi --mode rpc` as a",
        " child process and renders the same sessions, tools and models in a native",
        " window.",
        " .",
        " This is pre-release software. Conversations can modify project files;",
        " review agent actions and keep backups.",
        "",
    ]
    return "\n".join(lines)


def desktop_entry() -> str:
    return (
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Name=Dish\n"
        "Comment=Desktop interface for the Pi coding agent\n"
        "Exec=dish\n"
        "Icon=dish\n"
        "StartupWMClass=dish\n"
        "Terminal=false\n"
        "Categories=Development;\n"
    )


def copyright_text() -> str:
    return (
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        "Upstream-Name: dish\n"
        f"Source: {HOMEPAGE}\n"
        "\n"
        "Files: *\n"
        "Copyright: 2026 Cleison Santos\n"
        "License: Apache-2.0\n"
        " On Debian systems the complete license text is in /usr/share/common-licenses/Apache-2.0\n"
        " and a copy is included in LICENSE.\n"
    )


def stage_tree(package: Path, staging: Path, version: str, glibc: str, dependencies: list[str]) -> None:
    if not (package / "bin/dish").is_file():
        raise RuntimeError(f'Missing executable in {package}')
    (staging / "DEBIAN").mkdir(parents=True)
    (staging / "usr/bin").mkdir(parents=True)
    shutil.copyfile(package / "bin/dish", staging / "usr/bin/dish")
    (staging / "usr/bin/dish").chmod(0o755)
    (staging / "usr/share/applications").mkdir(parents=True)
    (staging / "usr/share/applications/dish.desktop").write_text(desktop_entry())
    icon_dir = staging / "usr/share/icons/hicolor/512x512/apps"
    icon_dir.mkdir(parents=True)
    shutil.copyfile(package / "share/icons/dish.png", icon_dir / "dish.png")
    docs = staging / "usr/share/doc/dish"
    (docs / "licenses").mkdir(parents=True)
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"]:
        shutil.copyfile(package / f"share/licenses/{name}", docs / name)
    for entry in (package / "share/licenses").iterdir():
        if entry.name in {"LICENSE", "THIRD_PARTY_NOTICES.md"}:
            continue
        destination = docs / "licenses" / entry.name
        if entry.is_dir():
            shutil.copytree(entry, destination)
        else:
            shutil.copyfile(entry, destination)
    shutil.copyfile(package / "README.md", docs / "README.md")
    (docs / "copyright").write_text(copyright_text())
    changelog = (
        f"dish ({version}) unstable; urgency=medium\n\n"
        "  * Linux preview release.\n\n"
        f" -- {MAINTAINER}  {email.utils.formatdate(localtime=False)}\n"
    )
    (docs / "changelog").write_text(changelog)
    installed_kb = sum(f.stat().st_size for f in staging.rglob("*") if f.is_file()) // 1024
    (staging / "DEBIAN/control").write_text(control_text(version, glibc, dependencies, installed_kb))


def build(archive: Path, output_dir: Path) -> Path:
    if shutil.which("dpkg-deb") is None or shutil.which("dpkg-query") is None:
        raise RuntimeError("dpkg-deb and dpkg-query are required to build a Debian package")
    checksum = Path(str(archive) + ".sha256")
    if checksum.is_file():
        expected = checksum.read_text().split()[0]
        if sha256(archive) != expected:
            raise RuntimeError(f"Checksum mismatch for {archive}; rebuild the candidate first")
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dish-deb-") as temporary:
        root = Path(temporary)
        with tarfile.open(archive) as tar:
            tar.extractall(root / "unpacked", filter="data")
        entries = [entry for entry in (root / "unpacked").iterdir() if entry.is_dir()]
        if len(entries) != 1:
            raise RuntimeError("The candidate archive must contain a single top-level directory")
        package = entries[0]
        build_json = json.loads((package / "BUILD.json").read_text())
        if build_json.get("target") != TARGET:
            raise RuntimeError(f'Unexpected build target: {build_json.get("target")}')
        glibc = str(build_json["minimum_glibc"])
        version = debian_version(build_json["version"])
        binary = package / "bin/dish"
        dependencies = linked_packages(binary)
        staging = root / "staging"
        stage_tree(package, staging, version, glibc, dependencies)
        output = output_dir / f"dish_{version}_{DEB_ARCH}.deb"
        subprocess.run(["dpkg-deb", "--build", "--root-owner-group", str(staging), str(output)],
                       check=True, cwd=ROOT)
    info = run("dpkg-deb", "--info", str(output))
    for expected in ["Package: dish", f"Version: {version}", f"Architecture: {DEB_ARCH}"]:
        if expected not in info:
            raise RuntimeError(f"Built package is missing {expected!r}")
    Path(str(output) + ".sha256").write_text(f"{sha256(output)}  {output.name}\n")
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", help="Candidate version label, e.g. 0.1.0-alpha.1")
    parser.add_argument("--input", help="Candidate tar.gz (defaults to dist/dish-<version>-linux-x86_64.tar.gz)")
    parser.add_argument("--output", default=str(ROOT / "dist"), help="Directory for the .deb and its checksum")
    args = parser.parse_args()
    base = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    version = args.version or base
    if not re.fullmatch(re.escape(base) + r"(?:-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)?", version):
        parser.error("Version must match Cargo.toml, optionally with a prerelease suffix")
    archive = Path(args.input) if args.input else ROOT / "dist" / f"dish-{version}-linux-x86_64.tar.gz"
    if not archive.is_file():
        parser.error(f"Candidate archive not found: {archive}")
    package = build(archive, Path(args.output))
    print(f"Debian package: {package} ({sha256(package)[:12]}…)")
    print("No upload performed; the package is published only by explicit release steps.")


if __name__ == "__main__":
    main()
