"""Offline checks for the `curl | sh` bootstrap installer; uses a local file:// release."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/install.sh"
TAG = "v0.1.0-test.1"
VERSION = TAG[1:]
ASSET = f"dish-{VERSION}-linux-x86_64.tar.gz"


@unittest.skipUnless(
    all(shutil.which(name) for name in ("curl", "tar", "sha256sum", "bash", "python3")),
    "bootstrap installer tools are not available",
)
class InstallScriptTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="dish-install-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.server = self.directory / "server" / TAG
        self.server.mkdir(parents=True)
        self.archive = self.server / ASSET
        self.build_archive()
        self.write_checksum()
        self.home = self.directory / "home"
        self.home.mkdir()
        self.fakebin = self.directory / "fakebin"
        self.fakebin.mkdir()
        self.env = os.environ.copy()
        self.env.pop("XDG_DATA_HOME", None)
        self.env.pop("DISH_INSTALL_API_URL", None)
        self.env["HOME"] = str(self.home)
        self.env["DISH_INSTALL_BASE_URL"] = self.server.parent.as_uri()
        self.env["PATH"] = str(self.fakebin) + os.pathsep + self.env["PATH"]

    def build_archive(self):
        top = f"dish-{VERSION}-linux-x86_64"
        entries = {
            f"{top}/bin/dish": (b"#!/bin/sh\necho fake dish\n", 0o755),
            f"{top}/share/icons/dish.png": (b"fake icon bytes\n", 0o644),
            f"{top}/share/licenses/LICENSE.txt": (b"fake license\n", 0o644),
            f"{top}/install.sh": ((ROOT / "scripts/install-release.sh").read_bytes(), 0o755),
            f"{top}/uninstall.sh": ((ROOT / "scripts/uninstall-release.sh").read_bytes(), 0o755),
            f"{top}/BUILD.json": (b"{}\n", 0o644),
        }
        with tarfile.open(self.archive, "w:gz") as archive:
            for name, (data, mode) in entries.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = mode
                info.mtime = 0
                archive.addfile(info, io.BytesIO(data))

    def write_checksum(self, digest=None, name=None):
        digest = digest or hashlib.sha256(self.archive.read_bytes()).hexdigest()
        name = name or self.archive.name
        self.archive.with_name(self.archive.name + ".sha256").write_text(
            f"{digest}  {name}\n")

    def run_installer(self, *args, stdin_script=False):
        if stdin_script:
            return subprocess.run(
                ["sh", "-s", "--", *args],
                input=SCRIPT.read_text(),
                capture_output=True,
                text=True,
                timeout=60,
                env=self.env,
                start_new_session=True,
            )
        return subprocess.run(
            ["sh", str(SCRIPT), *args],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
            env=self.env,
            start_new_session=True,
        )

    def installed_bin(self, root=None):
        return (root or self.home) / ".local/bin/dish"

    def assert_nothing_installed(self):
        self.assertFalse((self.home / ".local").exists())

    def test_installs_requested_version(self):
        result = self.run_installer("--version", TAG, "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.installed_bin()
        self.assertTrue(binary.is_file())
        self.assertTrue(os.access(binary, os.X_OK))
        self.assertTrue((self.home / ".local/share/applications/dish.desktop").is_file())
        self.assertTrue((self.home / ".local/share/dish/licenses/LICENSE.txt").is_file())

    def test_accepts_version_without_v(self):
        result = self.run_installer("--version", VERSION, "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())

    def test_piped_script(self):
        result = self.run_installer("--version", TAG, "--yes", stdin_script=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())

    def test_resolves_latest_from_api(self):
        releases = self.directory / "releases.json"
        releases.write_text(json.dumps([
            {"tag_name": TAG},
            {"tag_name": "v0.0.9"},
        ], indent=2) + "\n")
        self.env["DISH_INSTALL_API_URL"] = releases.as_uri()
        result = self.run_installer("--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())

    def test_wrong_checksum_aborts_without_installing(self):
        self.write_checksum(digest="0" * 64)
        result = self.run_installer("--version", TAG, "--yes")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checksum", result.stderr)
        self.assert_nothing_installed()

    def test_tampered_archive_aborts(self):
        with self.archive.open("ab") as handle:
            handle.write(b"tampered")
        result = self.run_installer("--version", TAG, "--yes")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Checksum", result.stderr)
        self.assert_nothing_installed()

    def test_missing_release_fails(self):
        result = self.run_installer("--version", "v9.9.9", "--yes")
        self.assertNotEqual(result.returncode, 0)
        self.assert_nothing_installed()

    def fake_uname(self, system, machine):
        path = self.fakebin / "uname"
        path.write_text(
            "#!/bin/sh\n"
            "case \"$1\" in\n"
            f"  -s) echo {system} ;;\n"
            f"  -m) echo {machine} ;;\n"
            f"  *) echo {system} ;;\n"
            "esac\n")
        path.chmod(0o755)

    def test_unsupported_platform(self):
        for system, machine in [("Linux", "aarch64"), ("Darwin", "x86_64")]:
            with self.subTest(system=system, machine=machine):
                self.fake_uname(system, machine)
                result = self.run_installer("--version", TAG, "--yes")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("x86_64", result.stderr)
                self.assert_nothing_installed()

    def test_refuses_root(self):
        path = self.fakebin / "id"
        path.write_text("#!/bin/sh\necho 0\n")
        path.chmod(0o755)
        result = self.run_installer("--version", TAG, "--yes")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("usuário", result.stderr)
        self.assert_nothing_installed()

    def test_prefix(self):
        prefix = self.directory / "custom"
        result = self.run_installer("--version", TAG, "--prefix", str(prefix), "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin(prefix).is_file())
        self.assert_nothing_installed()

        result = self.run_installer("--version", TAG, "--prefix", "relative", "--yes")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assert_nothing_installed()

    def test_uninstall(self):
        result = self.run_installer("--version", TAG, "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())

        result = self.run_installer("--version", TAG, "--uninstall", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.installed_bin().exists())
        self.assertFalse((self.home / ".local/share/applications/dish.desktop").exists())
        self.assertFalse(
            (self.home / ".local/share/icons/hicolor/512x512/apps/dish.png").exists())
        self.assertFalse((self.home / ".local/share/dish/licenses").exists())

        prefix = self.directory / "custom"
        result = self.run_installer("--version", TAG, "--prefix", str(prefix), "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin(prefix).is_file())
        result = self.run_installer("--version", TAG, "--prefix", str(prefix), "--uninstall", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.installed_bin(prefix).exists())

    def test_no_desktop(self):
        result = self.run_installer("--version", TAG, "--no-desktop", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())
        desktop = self.home / ".local/share/applications/dish.desktop"
        self.assertFalse(desktop.exists())

        desktop.parent.mkdir(parents=True, exist_ok=True)
        desktop.write_text("[Desktop Entry]\nName=Existing\n")
        result = self.run_installer("--version", TAG, "--no-desktop", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.installed_bin().is_file())
        self.assertTrue(desktop.exists())

    def test_invalid_version_rejected(self):
        result = self.run_installer("--version", "v1;rm", "--yes")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assert_nothing_installed()

    def test_help(self):
        result = self.run_installer("--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--prefix", result.stdout)

    def test_unknown_option(self):
        result = self.run_installer("--bogus")
        self.assertEqual(result.returncode, 2, result.stderr)

    def test_syntax(self):
        shells = ["bash"]
        if shutil.which("dash"):
            shells.append("dash")
        for shell in shells:
            with self.subTest(shell=shell):
                result = subprocess.run(
                    [shell, "-n", str(SCRIPT)], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
