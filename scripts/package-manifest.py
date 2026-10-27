#!/usr/bin/env python3
"""Generate the section 4 release manifest from existing Linux assets. No upload."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import tarfile

TARGET = "x86_64-unknown-linux-gnu"
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="Downloaded release assets and .sha256 files")
    parser.add_argument("--tag", required=True)
    parser.add_argument("--published-at", required=True)
    args = parser.parse_args()
    payload = manifest(args.directory, args.tag, args.published_at)
    # Fail rather than overwrite a manifest that may already have been signed.
    with (args.directory / "latest.json").open("xb") as destination:
        destination.write(payload)
    print("Generated latest.json. Sign its exact bytes before publishing.")


if __name__ == "__main__":
    main()
