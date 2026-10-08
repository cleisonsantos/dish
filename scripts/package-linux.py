#!/usr/bin/env python3
"""Build a Linux x86_64 candidate archive. Never uploads or publishes anything."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-gnu"
LICENSE_NAME = re.compile(r"^(licen[sc]e|copying|copyright|notice)([._-].*|$)", re.I)


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, **kwargs)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_environment():
    env = os.environ.copy()
    flags = env.get("CARGO_ENCODED_RUSTFLAGS")
    flags = flags.split("\x1f") if flags else shlex.split(env.get("RUSTFLAGS", ""))
    sysroot = run("rustc", "--print", "sysroot").strip()
    for source, destination in [(str(ROOT), "/dish"), (str(Path.home()), "/build/user"),
                                (sysroot, "/build/rust")]:
        flags.append(f"--remap-path-prefix={source}={destination}")
    # Registry caches can live outside HOME on CI or custom installations.
    cargo_home = Path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
    flags.append(f"--remap-path-prefix={cargo_home}=/build/cargo")
    env.pop("RUSTFLAGS", None)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env["CARGO_PROFILE_RELEASE_STRIP"] = "symbols"
    return env


def verify_binary(path):
    if path.read_bytes()[:4] != b"\x7fELF":
        raise RuntimeError("Release executable is not ELF")
    header = run("readelf", "-h", str(path))
    if "Advanced Micro Devices X86-64" not in header:
        raise RuntimeError("Release executable is not x86_64")
    strings = run("strings", str(path))
    # Scan line by line so path detections never print the matching private text.
    if any(re.search(r"/(?:home|Users)/[^/\s]+/|[A-Za-z]:\\Users\\", line) for line in strings.splitlines()):
        raise RuntimeError("Executable still contains personal home paths; artifact rejected")
    versions = run("readelf", "--version-info", str(path))
    glibc = set(re.findall(r"GLIBC_(\d+\.\d+(?:\.\d+)?)", versions))
    return max(glibc, key=lambda v: tuple(map(int, v.split('.')))) if glibc else None


def crate_archive(package, dest, lock):
    source_dir = Path(package["manifest_path"]).parent
    registry = source_dir.parent.name
    archive = source_dir.parents[2] / "cache" / registry / f'{package["name"]}-{package["version"]}.crate'
    entry = next((p for p in lock["package"] if p["name"] == package["name"]
                  and p["version"] == package["version"] and p.get("source") == package["source"]), None)
    if not entry or not archive.is_file() or sha256(archive) != entry.get("checksum"):
        raise RuntimeError(f'Cannot verify original source archive: {package["name"]} {package["version"]}')
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(archive, dest / archive.name)
    return archive.name


def collect_licenses(metadata, destination):
    overrides = {(p["name"], p["version"]): p for p in json.loads((ROOT / "licenses/overrides.json").read_text())}
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    native_path = ROOT / "licenses/native.json"
    native = {(p["name"], p["version"]): p for p in json.loads(native_path.read_text())} if native_path.exists() else {}
    packages = []
    for p in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if p["id"] == metadata["resolve"]["root"]:
            continue
        if not p.get("license"):
            raise RuntimeError(f'No declared license: {p["name"]}')
        source = Path(p["manifest_path"]).parent
        files = sorted(f for f in source.rglob("*") if f.is_file() and LICENSE_NAME.match(f.name))
        folder = destination / "dependencies" / f'{p["name"]}-{p["version"]}'
        override = None
        copied = []
        if files:
            for f in files:
                relative = f.relative_to(source)
                target = folder / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(str(relative))
        else:
            override = overrides.get((p["name"], p["version"]))
            if not override:
                raise RuntimeError(f'Missing license texts: {p["name"]} {p["version"]}; review licenses/overrides.json')
            for i, entry in enumerate(override["files"]):
                f = (ROOT / entry["path"]).resolve()
                if not f.is_relative_to(ROOT) or sha256(f) != entry["sha256"]:
                    raise RuntimeError(f'Invalid license override for {p["name"]}')
                target = folder / f'{i}-{f.name}'
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(target.name)
        native_entry = native.get((p["name"], p["version"]))
        if p["name"] == "freetype-sys" and not native_entry:
            raise RuntimeError("Review the new FreeType native license inputs in licenses/native.json")
        if native_entry:
            for entry in native_entry["files"]:
                f = (source / entry["path"]).resolve()
                if not f.is_relative_to(source) or sha256(f) != entry["sha256"]:
                    raise RuntimeError(f'Invalid native license input for {p["name"]}')
                target = folder / entry["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(entry["path"])
        record = {k: p.get(k) for k in ["name", "version", "license", "repository", "authors"]}
        record["files"] = copied
        if native_entry:
            record["native"] = native_entry
        if p["name"] == "self_cell":
            record["selected_license"] = "Apache-2.0"
        if override:
            record["override"] = override
        if p["name"] == "option-ext" or (override and override.get("include_source")) or (native_entry and native_entry.get("include_source")):
            record["source_archive"] = crate_archive(p, destination / "sources", lock)
        packages.append(record)
    (destination / "dependencies.json").write_text(json.dumps(packages, indent=2) + "\n")
    return len(packages)


def toolchain_licenses(destination):
    info = dict(line.split(": ", 1) for line in run("rustc", "-vV").splitlines() if ": " in line)
    commit = info.get("commit-hash", "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise RuntimeError("Cannot identify Rust toolchain source commit")
    folder = destination / "rust-toolchain"
    folder.mkdir()
    sources = []
    # Exact toolchain commit, never a moving branch. No remote code is executed.
    for name in ["LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT"]:
        url = f"https://raw.githubusercontent.com/rust-lang/rust/{commit}/{name}"
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
        (folder / name).write_bytes(data)
        sources.append({"file": name, "url": url, "sha256": hashlib.sha256(data).hexdigest()})
    documentation = Path(run("rustc", "--print", "sysroot").strip()) / "share/doc/rust"
    copyright_library = documentation / "COPYRIGHT-library.html"
    bundled_licenses = documentation / "licenses"
    if not copyright_library.is_file() or not bundled_licenses.is_dir():
        raise RuntimeError("Rust standard-library notices are missing; install the rust-docs component")
    shutil.copyfile(copyright_library, folder / "COPYRIGHT-library.html")
    shutil.copytree(bundled_licenses, folder / "licenses")
    sources.append({"file": "COPYRIGHT-library.html", "source": "installed Rust toolchain standard-library notices",
                    "sha256": sha256(folder / "COPYRIGHT-library.html")})
    (folder / "provenance.json").write_text(json.dumps({"rustc": info["release"], "files": sources}, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", help="Archive label, e.g. 0.1.0-alpha.1 (must match Cargo's base version)")
    parser.add_argument("--skip-build", action="store_true", help="Reuse the target-specific binary; privacy checks still run")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("Run on Linux x86_64")
    base = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    version = args.version or base
    if not re.fullmatch(re.escape(base) + r"(?:-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)?", version):
        parser.error("Archive version must match Cargo.toml, optionally with a prerelease suffix")
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--release", "--locked", "--target", TARGET], cwd=ROOT, env=build_environment(), check=True)
    binary = ROOT / "target" / TARGET / "release/dish"
    glibc = verify_binary(binary)
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--filter-platform", TARGET, "--format-version", "1"))
    name = f"dish-{version}-linux-x86_64"
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="dish-package-") as temporary:
        package = Path(temporary) / name
        (package / "bin").mkdir(parents=True)
        (package / "share/icons").mkdir(parents=True)
        legal = package / "share/licenses"
        legal.mkdir(parents=True)
        shutil.copyfile(binary, package / "bin/dish")
        (package / "bin/dish").chmod(0o755)
        shutil.copyfile(ROOT / "assets/dish-icon.png", package / "share/icons/dish.png")
        for file in ["LICENSE", "THIRD_PARTY_NOTICES.md"]:
            shutil.copyfile(ROOT / file, legal / file)
        count = collect_licenses(metadata, legal)
        toolchain_licenses(legal)
        for source, target in [("install-release.sh", "install.sh"), ("uninstall-release.sh", "uninstall.sh")]:
            shutil.copyfile(ROOT / "scripts" / source, package / target)
            (package / target).chmod(0o755)
        shutil.copyfile(ROOT / "docs/linux-release.md", package / "README.md")
        (package / "BUILD.json").write_text(json.dumps({"version": version, "target": TARGET, "minimum_glibc": glibc,
            "dependency_packages": count, "rustc": run("rustc", "--version").strip(),
            "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"), "icon_status": "provisional"}, indent=2) + "\n")
        archive = dist / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(package, arcname=name)
        (dist / f"{archive.name}.sha256").write_text(f"{sha256(archive)}  {archive.name}\n")
    print(f"Candidate package: {archive.relative_to(ROOT)} ({count} dependency license entries)")
    print(f"Required glibc: {glibc}. No upload performed; review the archive before publication.")


if __name__ == "__main__":
    main()
