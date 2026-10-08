"""Unit and installation tests for the Linux release tooling; no network or sudo."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_linux", ROOT / "scripts/package-linux.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
deb_spec = importlib.util.spec_from_file_location("package_deb", ROOT / "scripts/package-deb.py")
deb = importlib.util.module_from_spec(deb_spec)
deb_spec.loader.exec_module(deb)


class ReleaseToolTests(unittest.TestCase):
    def test_flags_preserve_existing_settings_and_remap_paths(self):
        with patch.dict(os.environ, {"RUSTFLAGS": "-C opt-level=2"}, clear=True), patch.object(package, "run", return_value="/toolchain\n"):
            env = package.build_environment()
        flags = env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
        self.assertEqual(flags[:2], ["-C", "opt-level=2"])
        self.assertTrue(any(f.startswith("--remap-path-prefix=") for f in flags))
        self.assertNotIn("RUSTFLAGS", env)
        self.assertEqual(env["CARGO_PROFILE_RELEASE_STRIP"], "symbols")

    def test_rejects_non_elf(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "dish"
            path.write_bytes(b"not an executable")
            with self.assertRaisesRegex(RuntimeError, "not ELF"):
                package.verify_binary(path)

    def test_binary_privacy_check_and_glibc_version(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "dish"
            path.write_bytes(b"\x7fELFtest")
            with patch.object(package, "run", side_effect=["Advanced Micro Devices X86-64", "/home/example/project/main.rs"]):
                with self.assertRaisesRegex(RuntimeError, "personal home paths"):
                    package.verify_binary(path)
            with patch.object(package, "run", side_effect=["Advanced Micro Devices X86-64", "/dish/src/main.rs", "GLIBC_2.9 GLIBC_2.35 GLIBC_2.3"]):
                self.assertEqual(package.verify_binary(path), "2.35")

    def test_missing_dependency_license_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "licenses").mkdir()
            (root / "licenses/overrides.json").write_text("[]")
            (root / "Cargo.lock").write_text("package = []")
            crate = root / "crate"
            crate.mkdir()
            (crate / "Cargo.toml").touch()
            metadata = {"resolve": {"root": "dish"}, "packages": [{"id": "missing", "name": "missing", "version": "1.0.0", "license": "MIT", "manifest_path": str(crate / "Cargo.toml")}]}
            with patch.object(package, "ROOT", root):
                with self.assertRaisesRegex(RuntimeError, "Missing license texts"):
                    package.collect_licenses(metadata, root / "output")

    def test_override_hash_is_checked(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "licenses").mkdir()
            (root / "licenses/overrides.json").write_text(json.dumps([{"name": "missing", "version": "1.0.0", "files": [{"path": "LICENSE", "sha256": "incorrect"}]}]))
            (root / "LICENSE").write_text("changed license")
            (root / "Cargo.lock").write_text("package = []")
            crate = root / "crate"
            crate.mkdir()
            (crate / "Cargo.toml").touch()
            metadata = {"resolve": {"root": "dish"}, "packages": [{"id": "missing", "name": "missing", "version": "1.0.0", "license": "MIT", "manifest_path": str(crate / "Cargo.toml")}]}
            with patch.object(package, "ROOT", root):
                with self.assertRaisesRegex(RuntimeError, "Invalid license override"):
                    package.collect_licenses(metadata, root / "output")

    def test_source_archive_checksum_is_checked(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            crate = root / "registry/src/index/test-1.0.0"
            crate.mkdir(parents=True)
            cache = root / "registry/cache/index"
            cache.mkdir(parents=True)
            archive = cache / "test-1.0.0.crate"
            archive.write_bytes(b"original source")
            p = {"name": "test", "version": "1.0.0", "source": "registry+test", "manifest_path": str(crate / "Cargo.toml")}
            entry = dict(p, checksum=package.sha256(archive))
            self.assertEqual(package.crate_archive(p, root / "output", {"package": [entry]}), archive.name)
            archive.write_bytes(b"tampered")
            with self.assertRaisesRegex(RuntimeError, "Cannot verify"):
                package.crate_archive(p, root / "output", {"package": [entry]})


@unittest.skipUnless(os.uname().sysname == "Linux" and os.uname().machine == "x86_64", "Linux x86_64 installer")
class InstallerTests(unittest.TestCase):
    def test_install_and_uninstall_preserve_user_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            app = root / "package with spaces"
            (app / "bin").mkdir(parents=True)
            (app / "bin/dish").write_text("#!/bin/sh\necho test\n")
            (app / "bin/dish").chmod(0o755)
            (app / "share/icons").mkdir(parents=True)
            (app / "share/icons/dish.png").write_bytes(b"icon")
            (app / "share/licenses").mkdir(parents=True)
            (app / "share/licenses/LICENSE").write_text("license")
            for source, target in [("install-release.sh", "install.sh"), ("uninstall-release.sh", "uninstall.sh")]:
                shutil.copyfile(ROOT / "scripts" / source, app / target)
            home = root / 'home $money%quote"and`back\\slash'
            data = root / "data with spaces"
            home.mkdir()
            prefs = home / ".config/dish/state.json"
            prefs.parent.mkdir(parents=True)
            prefs.write_text("{}")
            pi = data / "dish/pi/keep"
            pi.parent.mkdir(parents=True)
            pi.write_text("pi installation")
            env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(data))
            subprocess.run(["bash", str(app / "install.sh")], env=env, check=True, capture_output=True)
            self.assertTrue((home / ".local/bin/dish").is_file())
            self.assertTrue((data / "dish/licenses/LICENSE").is_file())
            desktop = (data / "applications/dish.desktop").read_text()
            for escaped in [r"\$", "%%", r'\"', r"\`", r"\\\\"]:
                self.assertIn(escaped, desktop)
            subprocess.run(["bash", str(app / "uninstall.sh")], env=env, check=True, capture_output=True)
            self.assertFalse((home / ".local/bin/dish").exists())
            self.assertFalse((data / "applications/dish.desktop").exists())
            self.assertFalse((data / "dish/licenses").exists())
            self.assertTrue(prefs.is_file())
            self.assertTrue(pi.is_file())


@unittest.skipUnless(os.environ.get("DISH_TEST_ARCHIVE"), "Set DISH_TEST_ARCHIVE to test a real candidate")
class CandidateArchiveTests(unittest.TestCase):
    def test_real_archive_integrity_licenses_and_installation(self):
        archive = Path(os.environ["DISH_TEST_ARCHIVE"]).resolve()
        checksum = Path(str(archive) + ".sha256").read_text().split()[0]
        self.assertEqual(package.sha256(archive), checksum)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            with tarfile.open(archive) as tar:
                tar.extractall(root / "unpacked", filter="data")
            app, = (root / "unpacked").iterdir()
            legal = app / "share/licenses"
            dependencies = json.loads((legal / "dependencies.json").read_text())
            build = json.loads((app / "BUILD.json").read_text())
            self.assertEqual(len(dependencies), build["dependency_packages"])
            for p in dependencies:
                directory = legal / "dependencies" / f'{p["name"]}-{p["version"]}'
                for file in p["files"]:
                    self.assertTrue((directory / file).is_file(), f'{p["name"]}: {file}')
            mpl = next(p for p in dependencies if p["name"] == "option-ext")
            self.assertTrue((legal / "sources" / mpl["source_archive"]).is_file())
            freetype = next(p for p in dependencies if p["name"] == "freetype-sys")
            self.assertEqual(freetype["native"]["selected_native_license"], "FTL")
            self.assertTrue((legal / "rust-toolchain/COPYRIGHT").is_file())
            self.assertTrue((legal / "rust-toolchain/COPYRIGHT-library.html").is_file())
            self.assertTrue((legal / "rust-toolchain/licenses").is_dir())
            home, data = root / "home", root / "data"
            home.mkdir()
            env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(data))
            subprocess.run(["bash", str(app / "install.sh")], env=env, check=True, capture_output=True)
            self.assertEqual(package.sha256(home / ".local/bin/dish"), package.sha256(app / "bin/dish"))
            self.assertTrue((data / "dish/licenses/sources" / mpl["source_archive"]).is_file())
            subprocess.run(["bash", str(app / "uninstall.sh")], env=env, check=True, capture_output=True)
            self.assertFalse((home / ".local/bin/dish").exists())


@unittest.skipUnless(shutil.which("dpkg-deb") and shutil.which("dpkg-query") and shutil.which("readelf"),
                     "Debian packaging tools (dpkg-deb, dpkg-query, readelf)")
class DebianPackageTests(unittest.TestCase):
    def test_debian_version_mapping_sorts_prereleases_below_releases(self):
        self.assertEqual(deb.debian_version("0.1.0-alpha.1"), "0.1.0~alpha.1")
        self.assertEqual(deb.debian_version("1.2.3-beta-2"), "1.2.3~beta.2")
        self.assertEqual(deb.debian_version("1.2.3"), "1.2.3")
        for invalid in ["", "1.2", "v1.2.3", "1.2.3-", "1.2.3-.."]:
            with self.assertRaises(ValueError):
                deb.debian_version(invalid)

    def test_control_pins_glibc_and_lists_runtime_recommendations(self):
        control = deb.control_text("0.1.0~alpha.1", "2.39", ["libc6", "libxcb1"], 100)
        self.assertIn("Version: 0.1.0~alpha.1", control)
        self.assertIn("Depends: libc6 (>= 2.39), libxcb1", control)
        self.assertIn("Architecture: amd64", control)
        self.assertIn("Recommends: libegl1, libvulkan1, libwayland-client0, libwayland-egl1", control)
        # Missing libc6 must be added rather than silently dropped.
        self.assertIn("libc6 (>= 2.39)", deb.control_text("1.0.0", "2.39", ["libgcc-s1"], 0))

    def test_builds_a_package_from_a_candidate_archive(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            name = "dish-0.1.0-alpha.1-linux-x86_64"
            staged = root / name
            (staged / "bin").mkdir(parents=True)
            shutil.copyfile("/bin/true", staged / "bin/dish")
            (staged / "share/icons").mkdir(parents=True)
            (staged / "share/icons/dish.png").write_bytes(b"icon")
            (staged / "share/licenses").mkdir(parents=True)
            (staged / "share/licenses/LICENSE").write_text("license")
            (staged / "share/licenses/THIRD_PARTY_NOTICES.md").write_text("notices")
            (staged / "share/licenses/dependencies").mkdir()
            (staged / "share/licenses/dependencies/entry.txt").write_text("dependency")
            (staged / "README.md").write_text("readme")
            (staged / "BUILD.json").write_text(json.dumps({
                "version": "0.1.0-alpha.1", "target": deb.TARGET, "minimum_glibc": "2.39",
                "dependency_packages": 1, "icon_status": "provisional"}))
            archive = root / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(staged, arcname=name)
            output = root / "dist"
            built = deb.build(archive, output)
            self.assertEqual(built.name, "dish_0.1.0-alpha.1_amd64.deb")
            self.assertEqual(built.parent, output)
            self.assertTrue(Path(str(built) + ".sha256").is_file())
            info = subprocess.check_output(["dpkg-deb", "--info", str(built)], text=True)
            self.assertIn("Version: 0.1.0~alpha.1", info)
            self.assertIn("Package: dish", info)
            listing = subprocess.check_output(["dpkg-deb", "--contents", str(built)], text=True)
            for member in ["usr/bin/dish", "usr/share/applications/dish.desktop",
                           "usr/share/icons/hicolor/512x512/apps/dish.png", "usr/share/doc/dish/copyright",
                           "usr/share/doc/dish/changelog", "usr/share/doc/dish/licenses/dependencies/entry.txt"]:
                self.assertIn(member, listing)
            # A sidecar checksum that no longer matches must stop the build.
            Path(str(archive) + ".sha256").write_text("0" * 64 + f"  {archive.name}\n")
            with self.assertRaisesRegex(RuntimeError, "Checksum mismatch"):
                deb.build(archive, output)

    def test_rejects_a_candidate_for_another_target(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            name = "dish-0.1.0-linux-aarch64"
            staged = root / name
            (staged / "bin").mkdir(parents=True)
            (staged / "bin/dish").write_bytes(b"x")
            (staged / "BUILD.json").write_text(json.dumps({
                "version": "0.1.0", "target": "aarch64-unknown-linux-gnu", "minimum_glibc": "2.39"}))
            archive = root / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(staged, arcname=name)
            with self.assertRaisesRegex(RuntimeError, "Unexpected build target"):
                deb.build(archive, root / "dist")


if __name__ == "__main__":
    unittest.main()
