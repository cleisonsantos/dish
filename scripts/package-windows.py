#!/usr/bin/env python3
"""Build the Windows installer payload and the NSIS setup executable.

The payload carries the same license bundle as the Linux release (issue #63):
the app, its two top-level notices, and the collected dependency/native/Rust
toolchain texts. NSIS turns that into `dish-<version>-windows-x86_64-setup.exe`.
Nothing here uploads or publishes.
"""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
import license_bundle  # noqa: E402  (precisa do sys.path acima)

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
PAYLOAD = ROOT / "dist/windows-payload"
VERSION_LABEL = r"[0-9][0-9A-Za-z.+_-]*"
LABEL = re.compile(rf"--version\s+['\"]?({VERSION_LABEL})")
ENV_LABEL = re.compile(rf"^\s*DISH_VERSION\s*:\s*['\"]?({VERSION_LABEL})", re.MULTILINE)


def run(*args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, encoding="utf-8", **kwargs)


def ci_release_version(ci: Path) -> str:
    """Return the single label declared in ci.yml, whichever spelling is used."""
    text = ci.read_text(encoding="utf-8")
    labels = list(dict.fromkeys(LABEL.findall(text) + ENV_LABEL.findall(text)))
    if not labels:
        raise RuntimeError(
            f"{ci} exists but defines no release label; expected "
            f"`--version <label>` or `DISH_VERSION: <label>`")
    if len(labels) > 1:
        raise RuntimeError(f"{ci} defines conflicting release labels: {', '.join(labels)}")
    return labels[0]


def release_version(explicit: str | None) -> str:
    """O rótulo vive no ci.yml; o Cargo.toml é o fallback apenas sem ci.yml."""
    if explicit:
        return explicit
    ci = ROOT / ".github/workflows/ci.yml"
    if ci.is_file():
        return ci_release_version(ci)
    return tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]


def numeric_version(label: str) -> str:
    """`0.1.0-alpha.3` -> `0.1.0.3`, como o VIProductVersion exige."""
    parts = [p for p in re.sub(r"[^0-9.]", ".", label.replace("-alpha.", ".")).split(".") if p]
    while len(parts) < 4:
        parts.append("0")
    return ".".join(parts[:4])


def stage_payload(version: str, target: str, require_binary: bool = True) -> int:
    binary = ROOT / "target/release/dish.exe"
    if not binary.is_file():
        if require_binary:
            raise RuntimeError(f"Missing executable: {binary}; run `cargo build --release` first")
        print(f"warning: {binary.name} ausente; empacotando só o pacote de licenças")
    if PAYLOAD.exists():
        shutil.rmtree(PAYLOAD)
    PAYLOAD.mkdir(parents=True)
    if binary.is_file():
        shutil.copyfile(binary, PAYLOAD / "dish.exe")
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"]:
        shutil.copyfile(ROOT / name, PAYLOAD / name)
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline",
                              "--filter-platform", target, "--format-version", "1"))
    count = license_bundle.build(metadata, PAYLOAD / "licenses", ROOT)
    (PAYLOAD / "BUILD.json").write_text(json.dumps({
        "version": version, "target": target, "dependency_packages": count,
        "rustc": run("rustc", "--version").strip(),
        "cargo_lock_sha256": license_bundle.sha256(ROOT / "Cargo.lock"),
    }, indent=2) + "\n", encoding="utf-8")
    return count


def find_makensis(explicit: str | None) -> str:
    if explicit:
        return explicit
    found = shutil.which("makensis")
    if found:
        return found
    candidates = [
        Path("C:/ProgramData/chocolatey/bin/makensis.exe"),
        Path("C:/Program Files (x86)/NSIS/makensis.exe"),
        Path("C:/Program Files/NSIS/makensis.exe"),
    ]
    for candidate in candidates:
        if candidate.is_file():
            return str(candidate)
    raise RuntimeError("makensis não encontrado; instale o NSIS (choco install nsis)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", help="Release label (default: ci.yml, then Cargo.toml)")
    parser.add_argument("--makensis", help="Path to makensis (default: PATH/common install dirs)")
    parser.add_argument("--bundle-only", action="store_true",
                        help="Only stage the payload (useful to verify the license bundle)")
    parser.add_argument("--target", default=TARGET,
                        help="Rust target used for the dependency inventory (default: %(default)s)")
    args = parser.parse_args()
    version = release_version(args.version)
    count = stage_payload(version, args.target, require_binary=not args.bundle_only)
    print(f"Payload: {PAYLOAD.relative_to(ROOT)} ({count} dependency license entries)")
    if args.bundle_only:
        return
    installer = ROOT / "dist" / f"dish-{version}-windows-x86_64-setup.exe"
    subprocess.run([find_makensis(args.makensis), "/V2", f"/DVERSION={version}",
                    f"/DVERSIONNUM={numeric_version(version)}", f"/DSRCDIR={ROOT}",
                    str(ROOT / "scripts/windows-installer.nsi")], cwd=ROOT, check=True)
    if not installer.is_file() or installer.stat().st_size < 1_000_000:
        raise RuntimeError(f"Installer missing or suspiciously small: {installer}")
    (installer.parent / (installer.name + ".sha256")).write_text(
        f"{license_bundle.sha256(installer)}  {installer.name}\n", encoding="utf-8")
    print(f"Installer: {installer.relative_to(ROOT)} "
          f"({installer.stat().st_size / 1_048_576:.1f} MB)")
    print("No upload performed; publish only through the release steps.")


if __name__ == "__main__":
    main()
