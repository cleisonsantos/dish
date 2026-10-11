#!/usr/bin/env python3
"""Collect the dependency license bundle shared by every Dish release.

The bundle is platform-independent: it walks the packages reported by
`cargo metadata` for the target being packaged, copies their license files
(plus the hash-pinned overrides and native notices), and adds the Rust
toolchain notices. Linux, Windows and macOS packaging all use this module so
the three releases carry the same obligations.
"""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import urllib.request

LICENSE_NAME = re.compile(r"^(licen[sc]e|copying|copyright|notice)([._-].*|$)", re.I)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def crate_archive(package, dest: Path, lock, root: Path) -> str:
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


def collect_licenses(metadata, destination: Path, root: Path) -> int:
    overrides = {(p["name"], p["version"]): p for p in json.loads((root / "licenses/overrides.json").read_text(encoding="utf-8"))}
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    native_path = root / "licenses/native.json"
    native = {(p["name"], p["version"]): p for p in json.loads(native_path.read_text(encoding="utf-8"))} if native_path.exists() else {}
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
                f = (root / entry["path"]).resolve()
                if not f.is_relative_to(root) or sha256(f) != entry["sha256"]:
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
            record["source_archive"] = crate_archive(p, destination / "sources", lock, root)
        packages.append(record)
    (destination / "dependencies.json").write_text(json.dumps(packages, indent=2) + "\n", encoding="utf-8")
    return len(packages)


def toolchain_licenses(destination: Path) -> None:
    """Rust toolchain and standard-library notices for the installed compiler."""
    def run(*args):
        return subprocess.check_output(args, text=True, encoding="utf-8")

    info = dict(line.split(": ", 1) for line in run("rustc", "-vV").splitlines() if ": " in line)
    commit = info.get("commit-hash", "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise RuntimeError("Cannot identify Rust toolchain source commit")
    folder = destination / "rust-toolchain"
    folder.mkdir(parents=True, exist_ok=True)
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
    shutil.copytree(bundled_licenses, folder / "licenses", dirs_exist_ok=True)
    sources.append({"file": "COPYRIGHT-library.html", "source": "installed Rust toolchain standard-library notices",
                    "sha256": sha256(folder / "COPYRIGHT-library.html")})
    (folder / "provenance.json").write_text(json.dumps({"rustc": info["release"], "files": sources}, indent=2) + "\n", encoding="utf-8")


def build(metadata, destination: Path, root: Path) -> int:
    """Collect everything a release archive must ship under `destination`."""
    destination.mkdir(parents=True, exist_ok=True)
    count = collect_licenses(metadata, destination, root)
    toolchain_licenses(destination)
    return count
