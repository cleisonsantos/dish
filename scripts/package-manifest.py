#!/usr/bin/env python3
"""Generate the section 4 release manifest from existing Linux assets. No upload."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

TARGET = "x86_64-unknown-linux-gnu"
SECRET_ENVIRONMENT = "MINISIGN_SECRET_KEY"
RELEASES = "https://github.com/cleisonsantos/dish/releases"
NUMBER = r"(?:0|[1-9][0-9]*)"
IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = re.compile(rf"{NUMBER}\.{NUMBER}\.{NUMBER}(?:-{IDENTIFIER}(?:\.{IDENTIFIER})*)?")
LIMIT = 64 * 1024


def validate_version(version):
    if not VERSION.fullmatch(version):
        raise ValueError("Expected exact SemVer without v or build metadata")
    return version


def checksum(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def checked_asset(directory, kind, name, tag):
    path = directory / name
    if path.is_symlink() or not path.is_file() or path.stat().st_size <= 0:
        raise ValueError(f"Missing or invalid release asset: {name}")
    digest = checksum(path)
    # Require the checksum to name this exact asset, not an unrelated file.
    fields = path.with_name(name + ".sha256").read_text(encoding="utf-8").split()
    if fields != [digest, name] and fields != [digest, "*" + name]:
        raise ValueError(f"Checksum mismatch: {name}")
    return {"kind": kind, "name": name, "sha256": digest,
            "size": path.stat().st_size, "url": f"{RELEASES}/download/{tag}/{name}"}


def build_info(archive, version):
    expected = f"dish-{version}-linux-x86_64/BUILD.json"
    with tarfile.open(archive, "r:gz") as package:
        matches = [entry for entry in package if entry.name == expected]
        if len(matches) != 1 or not matches[0].isfile() or matches[0].size > LIMIT:
            raise ValueError("Archive must contain one regular, bounded BUILD.json")
        with package.extractfile(matches[0]) as source:
            info = json.load(source)
    if info.get("version") != version or info.get("target") != TARGET:
        raise ValueError("BUILD.json version/target does not match release")
    glibc = info.get("minimum_glibc")
    if not isinstance(glibc, str) or not re.fullmatch(r"[0-9]+\.[0-9]+", glibc):
        raise ValueError("Invalid minimum_glibc in BUILD.json")
    return info


def manifest(directory, tag, published_at):
    if not tag.startswith("v"):
        raise ValueError("Release tag must start with v")
    version = validate_version(tag[1:])
    if not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|\+00:00)", published_at):
        raise ValueError("published_at must be an RFC 3339 timestamp in UTC")
    published = datetime.fromisoformat(published_at.replace("Z", "+00:00"))
    if "T" not in published_at or published.tzinfo is None or published.utcoffset().total_seconds() != 0:
        raise ValueError("published_at must be an RFC 3339 timestamp in UTC")
    published_at = published.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")
    portable_name = f"dish-{version}-linux-x86_64.tar.gz"
    assets = [checked_asset(directory, "portable-linux-x86_64", portable_name, tag),
              checked_asset(directory, "debian-amd64", f"dish-pi_{version}_amd64.deb", tag)]
    info = build_info(directory / portable_name, version)
    result = {"schema": 1, "version": version, "tag": tag,
              "channel": "prerelease" if "-" in version else "stable",
              "published_at": published_at, "notes_url": f"{RELEASES}/tag/{tag}",
              "target": TARGET, "minimum_glibc": info["minimum_glibc"], "assets": assets}
    encoded = (json.dumps(result, sort_keys=True, indent=2) + "\n").encode("utf-8")
    if len(encoded) > LIMIT:
        raise ValueError("Manifest exceeds 64 KiB")
    return encoded


def secret_key(secret_key_file=None, environment=None):
    """Return the secret key text from the environment or an explicit file path."""
    if secret_key_file is not None:
        try:
            value = Path(secret_key_file).read_text(encoding="utf-8")
        except OSError as error:
            raise ValueError(f"Cannot read secret key file: {secret_key_file}") from error
    else:
        source = os.environ if environment is None else environment
        value = source.get(SECRET_ENVIRONMENT)
    if value is not None and not value.strip():
        raise ValueError(f"{SECRET_ENVIRONMENT} is empty; refusing to sign")
    return value


def sign_manifest(manifest_path, key):
    """Sign the exact manifest bytes with minisign and write latest.json.minisig."""
    executable = shutil.which("minisign")
    if executable is None:
        raise RuntimeError("minisign was not found in PATH; cannot sign latest.json")
    manifest_path = Path(manifest_path)
    signature_path = manifest_path.with_name(manifest_path.name + ".minisig")
    # Keep the secret in a short-lived file with owner-only permissions. The
    # environment value is the key file content; it never enters the repository.
    with tempfile.TemporaryDirectory(prefix="dish-minisign-") as temporary:
        key_path = Path(temporary) / "release.key"
        descriptor = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as key_file:
            key_file.write(key if key.endswith("\n") else key + "\n")
        try:
            environment = os.environ.copy()
            # minisign reads the key from the temporary file; do not expose the
            # secret again through the child process environment.
            environment.pop(SECRET_ENVIRONMENT, None)
            result = subprocess.run(
                [executable, "-S", "-s", str(key_path), "-m", str(manifest_path),
                 "-x", str(signature_path)],
                capture_output=True, text=True, check=False,
                stdin=subprocess.DEVNULL, env=environment)
        except OSError as error:
            raise RuntimeError("Could not run minisign to sign latest.json") from error
    if result.returncode != 0:
        signature_path.unlink(missing_ok=True)
        detail = result.stderr.strip().splitlines()
        reason = detail[-1] if detail else "unknown error"
        raise RuntimeError(f"minisign failed to sign latest.json: {reason}")
    if not signature_path.is_file() or signature_path.stat().st_size == 0:
        raise RuntimeError("minisign did not create latest.json.minisig")
    return signature_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="Downloaded release assets and .sha256 files")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--published-at", required=True)
    parser.add_argument("--sign", "--release", dest="sign", action="store_true",
                        help="Sign latest.json in release mode (always required)")
    parser.add_argument("--secret-key-file", type=Path,
                        help=f"Minisign secret key path; overrides {SECRET_ENVIRONMENT}")
    args = parser.parse_args()
    try:
        key = secret_key(args.secret_key_file)
    except ValueError as error:
        parser.error(str(error))
    if key is None:
        parser.error(
            "refusing to generate an unsigned release manifest: set "
            f"{SECRET_ENVIRONMENT} or pass --secret-key-file")
    payload = manifest(args.directory, args.tag, args.published_at)
    destination = args.directory / "latest.json"
    # Fail rather than overwrite a manifest that may already have been signed.
    with destination.open("xb") as output:
        output.write(payload)
    try:
        signature = sign_manifest(destination, key)
    except Exception:
        # Never leave an unsigned manifest behind in release mode.
        destination.unlink(missing_ok=True)
        raise
    print(f"Generated latest.json and signed it as {signature.name}.")


if __name__ == "__main__":
    main()
