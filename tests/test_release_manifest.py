"""Offline checks for the section 4 manifest generator; no real release assets."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_manifest", ROOT / "scripts/package-manifest.py")
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-manifest-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.version = "0.1.0-alpha.6"
        self.tag = "v" + self.version
        self.published = "2026-10-26T12:00:00Z"
        self.portable = self.directory / f"dish-{self.version}-linux-x86_64.tar.gz"
        self.deb = self.directory / f"dish-pi_{self.version}_amd64.deb"
        self.archive()
        self.deb.write_bytes(b"fake deb: the generator hashes it, never installs it")
        self.sidecar(self.portable)
        self.sidecar(self.deb)

    def sidecar(self, path):
        path.with_name(path.name + ".sha256").write_text(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")

    def archive(self, **overrides):
        build = {"version": self.version, "target": generator.TARGET, "minimum_glibc": "2.39"}
        build.update(overrides)
        payload = json.dumps(build).encode()
        with tarfile.open(self.portable, "w:gz") as archive:
            entry = tarfile.TarInfo(f"dish-{self.version}-linux-x86_64/BUILD.json")
            entry.size = len(payload)
            archive.addfile(entry, io.BytesIO(payload))

    def generate(self):
        return generator.manifest(self.directory, self.tag, self.published)

    def test_schema_assets_checksums_and_deterministic_bytes(self):
        encoded = self.generate()
        self.assertEqual(encoded, self.generate())
        self.assertTrue(encoded.endswith(b"\n"))
        value = json.loads(encoded)
        self.assertEqual(value["schema"], 1)
        self.assertEqual(value["version"], self.version)
        self.assertEqual(value["tag"], self.tag)
        self.assertEqual(value["channel"], "prerelease")
        self.assertEqual(value["published_at"], self.published)
        self.assertEqual(value["notes_url"], f"{generator.RELEASES}/tag/{self.tag}")
        self.assertEqual(value["target"], generator.TARGET)
        self.assertEqual(value["minimum_glibc"], "2.39")
        self.assertEqual(len(value["assets"]), 2)
        self.assertEqual({asset["kind"] for asset in value["assets"]},
                         {"portable-linux-x86_64", "debian-amd64"})
        for asset in value["assets"]:
            path = self.directory / asset["name"]
            self.assertEqual(asset["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(asset["size"], path.stat().st_size)
            self.assertEqual(asset["url"], f"{generator.RELEASES}/download/{self.tag}/{path.name}")
        self.assertNotIn("latest.json", [asset["name"] for asset in value["assets"]])
        self.assertNotIn("latest.json.minisig", [asset["name"] for asset in value["assets"]])

    def test_invalid_versions(self):
        for version in ["v0.1.0", "0.1", "01.1.0", "0.1.0-alpha.01", "0.1.0+build.1", "0.1.0-", "../escape"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                generator.validate_version(version)
        for version in ["0.1.0", "0.1.0-alpha.10", "0.1.0-alpha-beta.1"]:
            self.assertEqual(generator.validate_version(version), version)

    def test_missing_asset_fails_closed(self):
        self.deb.unlink()
        with self.assertRaisesRegex(ValueError, "Missing or invalid"):
            self.generate()

    def test_checksum_mismatch_fails_closed(self):
        self.deb.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            self.generate()

    def test_checksum_must_name_the_same_asset(self):
        self.deb.with_name(self.deb.name + ".sha256").write_text(
            f"{hashlib.sha256(self.deb.read_bytes()).hexdigest()}  another.deb\n")
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            self.generate()

    def test_build_metadata_must_match_tag_and_target(self):
        for overrides in [{"version": "0.1.0-alpha.5"}, {"target": "aarch64-unknown-linux-gnu"},
                          {"minimum_glibc": "not-a-version"}]:
            with self.subTest(overrides=overrides):
                self.archive(**overrides)
                self.sidecar(self.portable)
                with self.assertRaises(ValueError):
                    self.generate()

    def test_timestamp_requires_utc(self):
        for timestamp in ["2026-10-26", "2026-10-26T12:00:00", "2026-10-26T12:00:00-03:00",
                          "20261026T12:00:00Z", "2026-10-26T12Z", "invalid"]:
            with self.subTest(timestamp=timestamp), self.assertRaises(ValueError):
                generator.manifest(self.directory, self.tag, timestamp)

    def test_symlink_asset_is_rejected(self):
        real = self.directory / "real.deb"
        self.deb.rename(real)
        self.deb.symlink_to(real)
        with self.assertRaisesRegex(ValueError, "Missing or invalid"):
            self.generate()


if __name__ == "__main__":
    unittest.main()
