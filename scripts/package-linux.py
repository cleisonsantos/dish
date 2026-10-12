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
import sys
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-linux-gnu"
sys.path.insert(0, str(Path(__file__).resolve().parent))
import license_bundle  # noqa: E402  (precisa do sys.path acima)


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8", **kwargs)


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


def collect_licenses(metadata, destination):
    """Mantém a API usada pelos testes; a lógica vive em `license_bundle`."""
    return license_bundle.collect_licenses(metadata, destination, ROOT)


def crate_archive(package, dest, lock):
    return license_bundle.crate_archive(package, dest, lock, ROOT)


def toolchain_licenses(destination):
    license_bundle.toolchain_licenses(destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", help="Archive label, e.g. 0.1.0-alpha.1 (must match Cargo's base version)")
    parser.add_argument("--skip-build", action="store_true", help="Reuse the target-specific binary; privacy checks still run")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("Run on Linux x86_64")
    base = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
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
        (dist / f"{archive.name}.sha256").write_text(f"{sha256(archive)}  {archive.name}\n", encoding="utf-8")
    print(f"Candidate package: {archive.relative_to(ROOT)} ({count} dependency license entries)")
    print(f"Required glibc: {glibc}. No upload performed; review the archive before publication.")


if __name__ == "__main__":
    main()
