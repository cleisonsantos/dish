"""Offline checks for the section 4 manifest generator; no real release assets."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_manifest", ROOT / "scripts/package-manifest.py")
generator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generator)


def write_sidecar(path):
    path.with_name(path.name + ".sha256").write_text(
        f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n")


def write_archive(portable, release_version, **overrides):
    build = {"version": release_version, "target": generator.TARGET, "minimum_glibc": "2.39"}
    build.update(overrides)
    payload = json.dumps(build).encode()
    with tarfile.open(portable, "w:gz") as archive:
        entry = tarfile.TarInfo(f"dish-{release_version}-linux-x86_64/BUILD.json")
        entry.size = len(payload)
        archive.addfile(entry, io.BytesIO(payload))


def stage_assets(directory, version):
    portable = directory / f"dish-{version}-linux-x86_64.tar.gz"
    deb = directory / f"dish-pi_{version}_amd64.deb"
    write_archive(portable, version)
    deb.write_bytes(b"fake deb: the generator hashes it, never installs it")
    write_sidecar(portable)
    write_sidecar(deb)
    return portable, deb


def release_asset(release, name):
    return next(asset for asset in release["assets"] if asset["name"] == name)


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-manifest-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.version = "0.1.0-alpha.6"
        self.tag = "v" + self.version
        self.published = "2026-10-26T12:00:00Z"
        self.portable, self.deb = stage_assets(self.directory, self.version)

    def sidecar(self, path):
        write_sidecar(path)

    def archive(self, **overrides):
        write_archive(self.portable, self.version, **overrides)

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


class ReleaseCheckTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-release-check-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.version = "0.1.0-alpha.6"
        self.tag = "v" + self.version
        self.published = "2026-10-26T12:00:00Z"
        self.portable, self.deb = stage_assets(self.directory, self.version)

    def generate(self):
        return generator.manifest(self.directory, self.tag, self.published)

    def release(self):
        value = json.loads(self.generate())
        assets = [{"name": asset["name"], "size": asset["size"],
                   "url": f"{generator.RELEASES}/download/{self.tag}/{asset['name']}",
                   "digest": "sha256:" + asset["sha256"]}
                  for asset in value["assets"]]
        for path in (self.portable, self.deb):
            sidecar = path.with_name(path.name + ".sha256")
            assets.append({"name": sidecar.name, "size": sidecar.stat().st_size,
                           "url": f"{generator.RELEASES}/download/{self.tag}/{sidecar.name}",
                           "digest": None})
        extra = f"dish-{self.version}-windows-x86_64-setup.exe"
        assets.append({"name": extra, "size": 123,
                       "url": f"{generator.RELEASES}/download/{self.tag}/{extra}",
                       "digest": None})
        return {"tagName": self.tag, "assets": assets}

    def check(self, release):
        return generator.check_release(self.generate(), release)

    def test_valid_release_passes(self):
        self.assertIsNone(self.check(self.release()))

    def test_missing_asset_fails(self):
        release = self.release()
        release["assets"].remove(release_asset(release, self.deb.name))
        with self.assertRaisesRegex(ValueError, f"Missing release asset: {self.deb.name}"):
            self.check(release)

    def test_duplicate_asset_fails(self):
        release = self.release()
        release["assets"].append(dict(release_asset(release, self.portable.name)))
        with self.assertRaisesRegex(ValueError, f"Duplicate release asset: {self.portable.name}"):
            self.check(release)

    def test_size_mismatch_fails(self):
        release = self.release()
        release_asset(release, self.portable.name)["size"] += 1
        with self.assertRaisesRegex(ValueError, f"Size mismatch: {self.portable.name}"):
            self.check(release)

    def test_url_mismatch_fails(self):
        release = self.release()
        release_asset(release, self.deb.name)["url"] = (
            f"{generator.RELEASES}/download/v0.0.1/{self.deb.name}")
        with self.assertRaisesRegex(ValueError, f"URL mismatch: {self.deb.name}"):
            self.check(release)

    def test_digest_mismatch_fails(self):
        release = self.release()
        release_asset(release, self.deb.name)["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(ValueError, f"Digest mismatch: {self.deb.name}"):
            self.check(release)

    def test_missing_digest_is_accepted(self):
        release = self.release()
        for asset in release["assets"]:
            asset["digest"] = None
        self.assertIsNone(self.check(release))

    def test_release_tag_mismatch_fails(self):
        release = self.release()
        release["tagName"] = "v9.9.9"
        with self.assertRaisesRegex(ValueError, "Release tag mismatch"):
            self.check(release)

    def test_manifest_missing_kind_fails(self):
        value = json.loads(self.generate())
        value["assets"] = [asset for asset in value["assets"]
                           if asset["kind"] != "debian-amd64"]
        with self.assertRaisesRegex(ValueError, "Manifest lacks asset kind: debian-amd64"):
            generator.check_release(json.dumps(value).encode(), self.release())

    def test_check_release_cli_needs_no_secret_key(self):
        manifest = self.directory / "latest.json"
        manifest.write_bytes(self.generate())
        release_path = self.directory / "release.json"
        release_path.write_text(json.dumps(self.release()), encoding="utf-8")
        environment = os.environ.copy()
        environment.pop(generator.SECRET_ENVIRONMENT, None)
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/package-manifest.py"),
             "--check-release", str(release_path), str(manifest)],
            capture_output=True, text=True, env=environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("latest.json matches the release assets.", result.stdout)


@unittest.skipUnless(shutil.which("minisign"), "minisign CLI is required to test signing")
class ManifestSigningTests(unittest.TestCase):
    version = "0.1.0-alpha.6"
    tag = "v" + version
    published = "2026-10-26T12:00:00Z"

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-manifest-sign-")
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.directory = root / "assets"
        self.directory.mkdir()
        self.keys = root / "keys"
        self.keys.mkdir()
        portable = self.directory / f"dish-{self.version}-linux-x86_64.tar.gz"
        deb = self.directory / f"dish-pi_{self.version}_amd64.deb"
        self.portable = portable
        self.deb = deb
        build = json.dumps({"version": self.version, "target": generator.TARGET,
                            "minimum_glibc": "2.39"}).encode()
        with tarfile.open(portable, "w:gz") as archive:
            entry = tarfile.TarInfo(f"dish-{self.version}-linux-x86_64/BUILD.json")
            entry.size = len(build)
            archive.addfile(entry, io.BytesIO(build))
        deb.write_bytes(b"fake deb: the generator hashes it, never installs it")
        for path in (portable, deb):
            write_sidecar(path)
        # Test-only key pair in the temporary directory; never copied to the repo.
        self.private = self.keys / "test.key"
        self.public = self.keys / "test.pub"
        subprocess.run([shutil.which("minisign"), "-G", "-W", "-s", str(self.private),
                        "-p", str(self.public)], check=True, capture_output=True, text=True)
        self.secret = self.private.read_text(encoding="utf-8")

    def run_generator(self, *arguments, environment_key=None):
        environment = os.environ.copy()
        environment.pop(generator.SECRET_ENVIRONMENT, None)
        if environment_key is not None:
            environment[generator.SECRET_ENVIRONMENT] = environment_key
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/package-manifest.py"),
             str(self.directory), "--tag", self.tag, "--published-at", self.published,
             *arguments], capture_output=True, text=True, env=environment)

    def verify(self):
        return subprocess.run(
            [shutil.which("minisign"), "-V", "-p", str(self.public),
             "-m", str(self.directory / "latest.json"),
             "-x", str(self.directory / "latest.json.minisig")],
            capture_output=True, text=True)

    def release(self):
        value = json.loads((self.directory / "latest.json").read_bytes())
        assets = [{"name": asset["name"], "size": asset["size"],
                   "url": f"{generator.RELEASES}/download/{self.tag}/{asset['name']}",
                   "digest": "sha256:" + asset["sha256"]}
                  for asset in value["assets"]]
        for path in sorted(self.directory.glob("*.sha256")):
            assets.append({"name": path.name, "size": path.stat().st_size,
                           "url": f"{generator.RELEASES}/download/{self.tag}/{path.name}",
                           "digest": None})
        extra = f"dish-{self.version}-windows-x86_64-setup.exe"
        assets.append({"name": extra, "size": 456,
                       "url": f"{generator.RELEASES}/download/{self.tag}/{extra}",
                       "digest": None})
        return {"tagName": self.tag, "assets": assets}

    def check(self, release):
        path = self.directory / "release.json"
        path.write_text(json.dumps(release), encoding="utf-8")
        environment = os.environ.copy()
        environment.pop(generator.SECRET_ENVIRONMENT, None)
        return subprocess.run(
            [sys.executable, str(ROOT / "scripts/package-manifest.py"),
             "--check-release", str(path), str(self.directory / "latest.json")],
            capture_output=True, text=True, env=environment)

    def test_environment_key_signs_exact_manifest_and_minisign_verifies(self):
        result = self.run_generator("--sign", environment_key=self.secret)
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.directory / "latest.json"
        signature = self.directory / "latest.json.minisig"
        self.assertTrue(manifest.is_file())
        self.assertTrue(signature.is_file())
        verification = self.verify()
        self.assertEqual(verification.returncode, 0, verification.stderr)
        manifest.write_bytes(manifest.read_bytes() + b" ")
        self.assertNotEqual(self.verify().returncode, 0, "signature must cover exact bytes")

    def test_secret_key_file_path_signs_and_verifies(self):
        result = self.run_generator("--secret-key-file", str(self.private))
        self.assertEqual(result.returncode, 0, result.stderr)
        verification = self.verify()
        self.assertEqual(verification.returncode, 0, verification.stderr)

    def test_missing_key_refuses_to_create_a_manifest(self):
        for arguments in [(), ("--sign",)]:
            with self.subTest(arguments=arguments):
                result = self.run_generator(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("refusing to generate an unsigned release manifest", result.stderr)
                self.assertFalse((self.directory / "latest.json").exists())
                self.assertFalse((self.directory / "latest.json.minisig").exists())

    def test_invalid_key_removes_the_unsigned_manifest(self):
        result = self.run_generator("--sign", environment_key="not a minisign key\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("minisign failed to sign latest.json", result.stderr)
        self.assertFalse((self.directory / "latest.json").exists())
        self.assertFalse((self.directory / "latest.json.minisig").exists())

    def test_release_check_accepts_signed_manifest_and_rejects_tampering(self):
        result = self.run_generator("--sign", environment_key=self.secret)
        self.assertEqual(result.returncode, 0, result.stderr)
        verification = self.verify()
        self.assertEqual(verification.returncode, 0, verification.stderr)
        checked = self.check(self.release())
        self.assertEqual(checked.returncode, 0, checked.stderr)
        self.assertIn("latest.json matches the release assets.", checked.stdout)

        release = self.release()
        release_asset(release, self.deb.name)["size"] += 4096
        checked = self.check(release)
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("Size mismatch", checked.stderr)

        release = self.release()
        release_asset(release, self.portable.name)["url"] = "https://example.invalid/elsewhere"
        checked = self.check(release)
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("URL mismatch", checked.stderr)

        release = self.release()
        release["assets"].remove(release_asset(release, self.deb.name))
        checked = self.check(release)
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("Missing release asset", checked.stderr)


@unittest.skipUnless(
    shutil.which("bash") and shutil.which("sha256sum") and shutil.which("minisign"),
    "bash, sha256sum and minisign are required to test the publish script")
class PublishReleaseScriptTests(unittest.TestCase):
    """End-to-end checks of scripts/publish-release.sh against a fake gh."""

    version = "0.1.0-alpha.6"
    tag = "v" + version

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-publish-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.script = ROOT / "scripts/publish-release.sh"
        self.bash = shutil.which("bash")
        self.minisign = shutil.which("minisign")

        self.artifact_dir = self.root / "artifact"
        self.artifact_dir.mkdir()
        self.portable, self.deb = stage_assets(self.artifact_dir, self.version)

        self.private, self.public = self.make_key_pair("keys")
        self.secret = self.private.read_text(encoding="utf-8")

        self.state_path = self.root / "state.json"
        self.log_path = self.root / "calls.log"
        self.storage = self.root / "storage"
        self.save_release({"tagName": self.tag, "isDraft": True,
                           "publishedAt": "", "assets": []})

        self.fake_gh = self.root / "gh"
        self.fake_gh.write_text(FAKE_GH, encoding="utf-8")
        self.fake_gh.chmod(0o755)

    def make_key_pair(self, name):
        directory = self.root / name
        directory.mkdir()
        private = directory / "test.key"
        public = directory / "test.pub"
        subprocess.run(
            [self.minisign, "-G", "-W", "-s", str(private), "-p", str(public)],
            check=True, capture_output=True, text=True)
        return private, public

    def release_state(self):
        return json.loads(self.state_path.read_text(encoding="utf-8"))

    def save_release(self, value):
        self.state_path.write_text(json.dumps(value), encoding="utf-8")

    def calls(self):
        if not self.log_path.exists():
            return []
        return self.log_path.read_text(encoding="utf-8").splitlines()

    def upload_calls(self):
        return [line for line in self.calls() if line.startswith("upload ")]

    def run_script(self, *arguments, key=None):
        environment = os.environ.copy()
        environment.pop(generator.SECRET_ENVIRONMENT, None)
        environment.pop("GH_REPO", None)
        environment.update({
            "GH": str(self.fake_gh),
            "FAKE_GH_STATE": str(self.state_path),
            "FAKE_GH_LOG": str(self.log_path),
            "MINISIGN": self.minisign,
            "PYTHON": sys.executable,
        })
        if key is not None:
            environment[generator.SECRET_ENVIRONMENT] = key
        return subprocess.run(
            [self.bash, str(self.script), self.tag, str(self.artifact_dir),
             "--public-key", str(self.public), *arguments],
            capture_output=True, text=True, env=environment)

    def test_happy_path_publishes_assets_then_signed_manifest_last(self):
        result = self.run_script(key=self.secret)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"Release {self.tag} completo", result.stdout)

        stored = {path.name for path in self.storage.iterdir()}
        self.assertEqual(stored, {
            self.portable.name, self.deb.name,
            self.portable.name + ".sha256", self.deb.name + ".sha256",
            "latest.json", "latest.json.minisig"})

        uploads = self.upload_calls()
        self.assertEqual(len(uploads), 5)
        self.assertEqual(uploads[-1], "upload latest.json latest.json.minisig")
        self.assertNotIn("--clobber", self.log_path.read_text(encoding="utf-8"))

        verification = subprocess.run(
            [self.minisign, "-V", "-p", str(self.public),
             "-m", str(self.storage / "latest.json"),
             "-x", str(self.storage / "latest.json.minisig")],
            capture_output=True, text=True)
        self.assertEqual(verification.returncode, 0, verification.stderr)

        manifest = json.loads((self.storage / "latest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["tag"], self.tag)
        self.assertEqual(manifest["published_at"], "2026-11-03T12:00:00Z")
        for asset in manifest["assets"]:
            self.assertEqual(
                asset["url"], f"{generator.RELEASES}/download/{self.tag}/{asset['name']}")

        state = self.release_state()
        self.assertFalse(state["isDraft"])
        self.assertEqual(state["publishedAt"], "2026-11-03T12:00:00Z")

    def test_missing_key_stops_before_any_github_call(self):
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("sem chave privada; nada foi publicado", result.stderr)
        self.assertEqual(self.calls(), [])
        self.assertFalse(self.storage.exists() and any(self.storage.iterdir()))

    def test_key_that_does_not_match_the_client_public_key_aborts(self):
        wrong_private, _ = self.make_key_pair("wrong-keys")
        result = self.run_script("--secret-key-file", str(wrong_private))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("não corresponde à chave pública", result.stderr)
        self.assertEqual(
            [line for line in self.calls() if line.startswith(("upload ", "edit "))], [])
        self.assertFalse(self.storage.exists() and any(self.storage.iterdir()))

    def test_existing_asset_with_different_size_prevents_manifest_upload(self):
        state = self.release_state()
        state["assets"].append({"name": self.portable.name,
                                "size": self.portable.stat().st_size + 1,
                                "digest": None})
        self.save_release(state)

        result = self.run_script(key=self.secret)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tamanho", result.stderr)
        self.assertEqual(self.upload_calls(), [])
        self.assertFalse((self.storage / "latest.json").exists())

    def test_release_that_already_has_a_manifest_is_never_overwritten(self):
        state = self.release_state()
        state["assets"].append({"name": "latest.json", "size": 1, "digest": None})
        self.save_release(state)

        result = self.run_script(key=self.secret)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("manifesto já publicado; nunca substituímos assets", result.stderr)
        self.assertEqual(self.upload_calls(), [])
        self.assertFalse((self.storage / "latest.json").exists())

    def test_invalid_local_checksum_stops_before_any_github_call(self):
        self.deb.write_bytes(b"changed after the sidecar was written")
        result = self.run_script(key=self.secret)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum inválido", result.stderr)
        self.assertEqual(self.calls(), [])
        self.assertFalse(self.storage.exists() and any(self.storage.iterdir()))

    def test_archive_refused_by_generator_stops_before_any_github_call(self):
        write_archive(self.portable, self.version, version="0.0.9")
        write_sidecar(self.portable)
        result = self.run_script(key=self.secret)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("nada foi publicado", result.stderr)
        self.assertEqual(self.calls(), [])
        self.assertFalse(self.storage.exists() and any(self.storage.iterdir()))


FAKE_GH = '''#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

STATE = Path(os.environ["FAKE_GH_STATE"])
LOG = Path(os.environ["FAKE_GH_LOG"])
STORAGE = STATE.parent / "storage"
STORAGE.mkdir(exist_ok=True)


def load():
    return json.loads(STATE.read_text(encoding="utf-8"))


def save(state):
    STATE.write_text(json.dumps(state), encoding="utf-8")


def log(*parts):
    with LOG.open("a", encoding="utf-8") as output:
        output.write(" ".join(parts) + "\\n")


def fail(message):
    print("fake gh: " + message, file=sys.stderr)
    sys.exit(1)


arguments = sys.argv[1:]
if arguments[:1] != ["release"]:
    fail("expected release")
arguments = arguments[1:]
if "--repo" in arguments:
    index = arguments.index("--repo")
    del arguments[index:index + 2]
if not arguments:
    fail("missing command")
command = arguments.pop(0)
state = load()
tag = state["tagName"]

if command == "view":
    if not arguments or arguments.pop(0) != tag:
        fail("unknown release")
    while arguments:
        option = arguments.pop(0)
        if option == "--json" and arguments:
            arguments.pop(0)
        else:
            fail("unexpected view option " + option)
    prefix = "untagged-x" if state.get("isDraft") else tag
    assets = [
        {"name": asset["name"], "size": asset["size"],
         "url": "https://github.com/cleisonsantos/dish/releases/download/"
                + prefix + "/" + asset["name"],
         "digest": asset.get("digest")}
        for asset in state.get("assets") or []
    ]
    log("view", tag)
    print(json.dumps({"tagName": tag, "isDraft": bool(state.get("isDraft")),
                      "publishedAt": state.get("publishedAt") or "",
                      "assets": assets}))
elif command == "upload":
    if not arguments or arguments.pop(0) != tag:
        fail("unknown release for upload")
    if not arguments:
        fail("upload needs files")
    names = []
    for filename in arguments:
        source = Path(filename)
        if not source.is_file():
            fail("missing file " + filename)
        if (STORAGE / source.name).exists():
            fail("asset already exists: " + source.name)
        shutil.copyfile(source, STORAGE / source.name)
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        state.setdefault("assets", []).append(
            {"name": source.name, "size": source.stat().st_size,
             "digest": "sha256:" + digest})
        names.append(source.name)
    save(state)
    log("upload", *names)
elif command == "edit":
    if not arguments or arguments.pop(0) != tag:
        fail("unknown release for edit")
    if "--draft=false" not in arguments:
        fail("only --draft=false is supported")
    state["isDraft"] = False
    state["publishedAt"] = "2026-11-03T12:00:00Z"
    save(state)
    log("edit", tag, "--draft=false")
elif command == "download":
    if not arguments or arguments.pop(0) != tag:
        fail("unknown release for download")
    directory = None
    name = None
    while arguments:
        option = arguments.pop(0)
        if option in ("--dir", "-D") and arguments:
            directory = Path(arguments.pop(0))
        elif option in ("-p", "--pattern") and arguments:
            name = arguments.pop(0)
        else:
            fail("unexpected download option " + option)
    if directory is None or name is None:
        fail("download needs --dir and -p")
    source = STORAGE / name
    if not source.is_file():
        fail("asset not found: " + name)
    directory.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, directory / name)
    log("download", tag, name)
else:
    fail("unknown command " + command)
'''


if __name__ == "__main__":
    unittest.main()
