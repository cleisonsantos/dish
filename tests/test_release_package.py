"""Unit and installation tests for the Linux release tooling; no network or sudo."""
import hashlib
import importlib.util
import io
import json
import os
import re
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import tarfile
import unittest
from unittest.mock import patch
import urllib.error
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_linux", ROOT / "scripts/package-linux.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
deb_spec = importlib.util.spec_from_file_location("package_deb", ROOT / "scripts/package-deb.py")
deb = importlib.util.module_from_spec(deb_spec)
deb_spec.loader.exec_module(deb)
win_spec = importlib.util.spec_from_file_location("package_windows", ROOT / "scripts/package-windows.py")
windows = importlib.util.module_from_spec(win_spec)
win_spec.loader.exec_module(windows)
# `package-linux.py` já colocou scripts/ em sys.path ao ser importado acima.
import license_bundle  # noqa: E402


class ReleaseToolTests(unittest.TestCase):
    def test_reused_binary_must_match_the_release_build_identity(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(package, "ROOT", Path(temporary)):
            binary = package.ROOT / "target" / package.TARGET / "release/dish"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"test release executable")
            with self.assertRaisesRegex(RuntimeError, "identity missing"):
                package.build_release("0.1.0-alpha.6", True)
            with patch.object(package, "build_environment", return_value={"DISH_RELEASE_VERSION": "0.1.0-alpha.6"}), patch.object(package.subprocess, "run") as build:
                self.assertEqual(package.build_release("0.1.0-alpha.6", False), binary)
                self.assertEqual(build.call_args.kwargs["env"]["DISH_RELEASE_VERSION"], "0.1.0-alpha.6")
            self.assertEqual(package.build_release("0.1.0-alpha.6", True), binary)
            with self.assertRaisesRegex(RuntimeError, "identity differs"):
                package.build_release("0.1.0-alpha.7", True)
            binary.write_bytes(b"another build")
            with self.assertRaisesRegex(RuntimeError, "identity differs"):
                package.build_release("0.1.0-alpha.6", True)

    def test_release_label_is_injected_without_changing_cargo_version(self):
        with patch.dict(os.environ, {"DISH_RELEASE_VERSION": "wrong-label"}, clear=True), patch.object(package, "run", return_value="/toolchain\n"):
            env = package.build_environment("0.1.0-alpha.6")
        self.assertEqual(env["DISH_RELEASE_VERSION"], "0.1.0-alpha.6")
        self.assertEqual(__import__("tomllib").loads((ROOT / "Cargo.toml").read_text())["package"]["version"], "0.1.0")

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
            installed = subprocess.run(["bash", str(app / "install.sh")], env=env,
                                      check=True, capture_output=True, stdin=subprocess.DEVNULL)
            # Sem terminal o script instala direto, sem menu.
            self.assertIn("Sem terminal interativo", installed.stdout.decode())
            self.assertTrue((home / ".local/bin/dish").is_file())
            self.assertTrue((data / "dish/licenses/LICENSE").is_file())
            desktop = (data / "applications/dish.desktop").read_text()
            for escaped in [r"\$", "%%", r'\"', r"\`", r"\\\\"]:
                self.assertIn(escaped, desktop)
            subprocess.run(["bash", str(app / "uninstall.sh")], env=env,
                           check=True, capture_output=True, stdin=subprocess.DEVNULL)
            self.assertFalse((home / ".local/bin/dish").exists())
            self.assertFalse((data / "applications/dish.desktop").exists())
            self.assertFalse((data / "dish/licenses").exists())
            self.assertTrue(prefs.is_file())
            self.assertTrue(pi.is_file())

    def test_install_script_uninstall_option_needs_no_payload(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            app = root / "package"
            app.mkdir()
            for source, target in [("install-release.sh", "install.sh"), ("uninstall-release.sh", "uninstall.sh")]:
                shutil.copyfile(ROOT / "scripts" / source, app / target)
            home, data = root / "home", root / "data"
            (home / ".local/bin").mkdir(parents=True)
            (home / ".local/bin/dish").write_text("installed")
            (data / "applications").mkdir(parents=True)
            (data / "applications/dish.desktop").write_text("[Desktop Entry]")
            env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(data))
            # Sem bin/ no pacote: desinstalar não depende do payload.
            subprocess.run(["bash", str(app / "install.sh"), "uninstall"], env=env,
                           check=True, capture_output=True, stdin=subprocess.DEVNULL)
            self.assertFalse((home / ".local/bin/dish").exists())
            self.assertFalse((data / "applications/dish.desktop").exists())

    def test_install_script_rejects_unknown_options(self):
        result = subprocess.run(["bash", str(ROOT / "scripts/install-release.sh"), "--turbo"],
                                capture_output=True, text=True, stdin=subprocess.DEVNULL)
        self.assertEqual(result.returncode, 2)
        self.assertIn("Uso:", result.stderr)

    def test_install_script_help_changes_nothing(self):
        with tempfile.TemporaryDirectory() as temp:
            home = Path(temp) / "home"
            home.mkdir()
            env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(Path(temp) / "data"))
            result = subprocess.run(["bash", str(ROOT / "scripts/install-release.sh"), "--help"],
                                    env=env, capture_output=True, text=True, stdin=subprocess.DEVNULL)
            self.assertEqual(result.returncode, 0)
            self.assertIn("Uso:", result.stdout)
            self.assertFalse((home / ".local/bin").exists())


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
            subprocess.run(["bash", str(app / "install.sh")], env=env, check=True,
                           capture_output=True, stdin=subprocess.DEVNULL)
            self.assertEqual(package.sha256(home / ".local/bin/dish"), package.sha256(app / "bin/dish"))
            self.assertTrue((data / "dish/licenses/sources" / mpl["source_archive"]).is_file())
            subprocess.run(["bash", str(app / "uninstall.sh")], env=env, check=True,
                           capture_output=True, stdin=subprocess.DEVNULL)
            self.assertFalse((home / ".local/bin/dish").exists())


class WindowsPackagingTests(unittest.TestCase):
    def test_numeric_version_for_viproductversion(self):
        self.assertEqual(windows.numeric_version("0.1.0-alpha.3"), "0.1.0.3")
        self.assertEqual(windows.numeric_version("0.1.0"), "0.1.0.0")
        self.assertEqual(windows.numeric_version("1.2.3-beta.4"), "1.2.3.4")

    def test_release_label_comes_from_ci_then_cargo(self):
        self.assertEqual(windows.release_version("9.9.9"), "9.9.9")
        label = windows.release_version(None)
        self.assertIn(label, (ROOT / ".github/workflows/ci.yml").read_text())

    def test_windows_build_uses_the_release_label_from_ci(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text(encoding="utf-8")
        lines = workflow.splitlines()
        build = next(i for i, line in enumerate(lines)
                     if line.strip() == "run: cargo build --release --locked")
        exports = [i for i, line in enumerate(lines[:build])
                   if "DISH_RELEASE_VERSION=" in line and "GITHUB_ENV" in line]
        self.assertTrue(exports,
                        "windows.yml precisa exportar DISH_RELEASE_VERSION antes do build")
        start = max(i for i in range(exports[0]) if re.match(r"\s+- name:", lines[i]))
        step = "\n".join(lines[start:exports[0] + 1])
        # O rótulo vem do ci.yml pela função do empacotador, nunca escrito à mão.
        self.assertIn("package-windows.py", step)
        self.assertIn("release_version(None)", step)
        self.assertNotRegex(step, r"\d+\.\d+")
        # Rótulo vazio reproduz o defeito: o passo precisa falhar cedo.
        self.assertIn("set -euo pipefail", step)
        self.assertIn("if [ -z", step)

    def test_installer_ships_the_shared_license_bundle(self):
        nsi = (ROOT / "scripts/windows-installer.nsi").read_text()
        self.assertIn('File /r "${SRCDIR}\\dist\\windows-payload\\licenses"', nsi)
        self.assertIn('RMDir /r "$INSTDIR\\licenses"', nsi)

    def test_msi_version_is_derived_and_passed_to_the_builder(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text()
        resolve = workflow.split("name: Resolve the release label", 1)[1].split("      - name:", 1)[0]
        self.assertIn("release_version(None)", resolve)
        self.assertIn("m.numeric_version(os.environ['RELEASE_LABEL'])", resolve)
        self.assertIn('RELEASE_LABEL="$version" python', resolve)
        self.assertIn('echo "numeric=$numeric" >> "$GITHUB_OUTPUT"', resolve)
        self.assertNotRegex(resolve, r"\d+\.\d+")
        build = workflow.split("name: Build the MSI installer", 1)[1].split("      - name:", 1)[0]
        self.assertIn("MSI_VERSION: ${{ steps.release-version.outputs.numeric }}", build)
        self.assertIn("RELEASE_LABEL: ${{ steps.release-version.outputs.label }}", build)
        self.assertIn('package-msi.ps1 -Version $env:MSI_VERSION', build)
        self.assertIn('-OutputPath "dist/dish-$env:RELEASE_LABEL-windows-x86_64.msi"', build)
        self.assertNotRegex(build, r"\d+\.\d+")
        script = (ROOT / "scripts/package-msi.ps1").read_text()
        self.assertIn('[Parameter(Mandatory)][string]$Version', script)
        self.assertIn('[Parameter(Mandatory)][string]$OutputPath', script)
        self.assertIn('Version="$Version"', script)
        self.assertNotRegex(script, r'Version="[0-9]')
        self.assertIn("'^[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+$'", script)
        self.assertIn("throw 'MSI Version must be numeric with four parts", script)
        self.assertIn("throw 'WiX not found", script)

    def test_msi_upgrade_identity_is_fixed_across_builds(self):
        script = (ROOT / "scripts/package-msi.ps1").read_text()
        # Pin the product family's identity: changing this breaks installed upgrades.
        codes = re.findall(r"^\$UpgradeCode\s*=\s*'([^']+)'", script, re.MULTILINE)
        self.assertEqual(codes, ["8B5C069E-5489-43A7-92FC-A668810CD0DB"])
        self.assertEqual(len(re.findall(r"\$UpgradeCode\s*=", script)), 1)
        self.assertNotRegex(script.lower(), r"newguid|new-guid|uuid4|guidgen")
        self.assertIn('UpgradeCode="$UpgradeCode"', script)
        self.assertIn('<MajorUpgrade AllowSameVersionUpgrades="yes"', script)
        self.assertIn('Schedule="afterInstallInitialize"', script)
        self.assertNotIn('IgnoreRemoveFailure="yes"', script)

    def test_msi_component_namespace_and_logical_names_are_stable(self):
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        namespaces = re.findall(r"^\$ComponentGuidNamespace\s*=\s*'([^']+)'", script, re.MULTILINE)
        self.assertEqual(namespaces, ['E2829A7B-5D29-4FCC-989B-73EB8D15336E'])
        self.assertEqual(len(re.findall(r'\$ComponentGuidNamespace\s*=', script)), 1)
        helper = script.split('function Get-ComponentGuid', 1)[1].split('$componentRefs =', 1)[0]
        for unstable in ('$Version', '$OutputPath', '$root', '$payload', 'GetRandomFileName', 'NewGuid'):
            self.assertNotIn(unstable, helper)
        for logical_name in ('file/$RelativePath', 'directory/$RelativePath', 'directory/install',
                             'shortcut/start-menu', 'shortcut/desktop'):
            self.assertIn('Get-ComponentGuid ' + ('"' + logical_name + '"' if '$' in logical_name
                                                else "'" + logical_name + "'"), script)

    def test_msi_templates_use_reviewed_wix4_elements_and_attributes(self):
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        # Reviewed subset of WiX 4's schema, not a full schema validator:
        # https://docs.firegiant.com/wix/schema/wxs/feature/
        # https://github.com/wixtoolset/web/blob/master/src/xsd4/wix.xsd
        # Check every XML opening tag, including inline strings and here-strings.
        # Newly used elements/attributes require review before joining this list.
        allowed = {
            'Wix': {'xmlns', 'xmlns:ui'},
            'Package': {'Name', 'Manufacturer', 'Version', 'UpgradeCode', 'Scope', 'Language'},
            'Component': {'Id', 'Guid'},
            'RegistryValue': {'Root', 'Key', 'Name', 'Type', 'Value', 'KeyPath'},
            'File': {'Id', 'Source', 'Name'},
            'RemoveFolder': {'Id', 'Directory', 'On'},
            'Directory': {'Id', 'Name'},
            'MajorUpgrade': {'AllowSameVersionUpgrades', 'Schedule', 'DowngradeErrorMessage'},
            'MediaTemplate': {'EmbedCab'},
            'Icon': {'Id', 'SourceFile'},
            'Property': {'Id', 'Value'},
            'StandardDirectory': {'Id'},
            'Shortcut': {'Id', 'Name', 'Target', 'WorkingDirectory', 'Icon'},
            'Feature': {'Id', 'Title', 'Level', 'AllowAbsent', 'AllowAdvertise', 'Display', 'Description'},
            'ComponentRef': {'Id'},
            'ui:WixUI': {'Id'},
            'WixVariable': {'Id', 'Value'},
        }
        tags = re.findall(r'<([A-Za-z_][\w:.-]*)\b((?:"[^"]*"|[^">])*)>',
                          script.replace('`"', '"'))
        self.assertTrue(tags, 'No MSI XML templates found')
        self.assertEqual({tag for tag, _ in tags}, set(allowed))
        for tag, attributes in tags:
            with self.subTest(element=tag, attributes=attributes):
                names = set(re.findall(r'([\w:.-]+)\s*=', attributes))
                self.assertFalse(names - allowed[tag], f'Unreviewed WiX 4 attributes on {tag}: {names - allowed[tag]}')
                if tag == 'Feature':
                    self.assertNotRegex(attributes, r'\bAbsent\s*=')
                    for attribute in ('AllowAbsent', 'AllowAdvertise'):
                        value = re.search(r'\b' + attribute + r'="([^"]*)"', attributes)
                        if value:
                            self.assertIn(value.group(1), ('yes', 'no'))
        self.assertIn('AllowAbsent="no"', script)
        self.assertNotIn('http://schemas.microsoft.com/wix/2006/wi', script)
        # Old sequence conditions were element text; v4 uses Condition=.
        self.assertNotRegex(script, r'<(?:Custom|RemoveExistingProducts)\b[^>]*>\s*[^<\s]')

    def test_msi_user_profile_components_have_explicit_stable_guids(self):
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        components = re.findall(r'<Component\s+([^>]+)>(.*?)</Component>', script, re.DOTALL)
        self.assertTrue(components)
        for attributes, body in components:
            with self.subTest(component=attributes):
                self.assertNotIn('Guid="*"', attributes)
                self.assertIn('Root="HKCU"', body)
                self.assertIn('KeyPath="yes"', body)
        file_builder = script.split('function Add-PayloadFile', 1)[1].split('function Add-PayloadDirectory', 1)[0]
        self.assertIn('Get-ComponentGuid "file/$RelativePath"', file_builder)
        self.assertIn('Guid="$guid"', file_builder)
        payload = re.search(r'<Component\s+[^>]+>(.*?)</Component>', file_builder, re.DOTALL).group(1)
        self.assertEqual(len(re.findall(r'<File\s', payload)), 1)
        self.assertRegex(payload, r'<RegistryValue\s+[^>]*Root="HKCU"[^>]*KeyPath="yes"')

    def test_windows_compares_two_independently_generated_msi_sources(self):
        workflow = (ROOT / '.github/workflows/windows.yml').read_text()
        build = workflow.split('name: Build the MSI installer', 1)[1].split('      - name:', 1)[0]
        guard = workflow.split('name: Verify MSI source determinism', 1)[1].split('      - name:', 1)[0]
        self.assertIn('-SourcePath "$env:RUNNER_TEMP/dish-msi-first/Dish.wxs"', build)
        self.assertIn('$first = "$env:RUNNER_TEMP/dish-msi-first/Dish.wxs"', guard)
        self.assertIn('$second = "$env:RUNNER_TEMP/dish-msi-second/Dish.wxs"', guard)
        self.assertIn('package-msi.ps1 -Version $env:MSI_VERSION', guard)
        self.assertIn('-SourcePath $second -GenerateOnly', guard)
        self.assertIn('MSI_VERSION: ${{ steps.release-version.outputs.numeric }}', guard)
        for path in ('$first', '$second'):
            self.assertIn('Get-FileHash -LiteralPath ' + path + ' -Algorithm SHA256', guard)
            self.assertIn('[IO.File]::ReadAllBytes(' + path + ')', guard)
        self.assertIn('if ($firstHash -cne $secondHash) { throw', guard)
        self.assertIn('Compare-Object -ReferenceObject', guard)
        self.assertIn('-SyncWindow 0', guard)
        self.assertIn('if ($differences) { throw', guard)
        self.assertLess(workflow.index('name: Build the MSI installer'),
                        workflow.index('name: Verify MSI source determinism'))
        self.assertLess(workflow.index('name: Verify MSI source determinism'),
                        workflow.index('name: Sign and verify the MSI installer'))
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        self.assertIn('Id="WixUILicenseRtf" Value="license.rtf"', script)
        self.assertIn('-b $work', script)
        # Keep the actual WiX build's ICE validation enabled.
        for text in (workflow, script):
            self.assertNotRegex(text.lower(), r'-(?:sice|sval|suppressvalidation)\b')

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required to generate MSI source')
    def test_msi_generation_is_identical_in_different_temporary_directories(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            scripts = root / 'scripts'
            scripts.mkdir()
            shutil.copyfile(ROOT / 'scripts/package-msi.ps1', scripts / 'package-msi.ps1')
            (scripts / 'install-pi-windows.ps1').write_text('# fixture')
            (root / 'assets').mkdir()
            (root / 'assets/dish-icon.ico').write_bytes(b'fixture')
            payload = root / 'dist/windows-payload'
            licenses = payload / 'licenses/example'
            licenses.mkdir(parents=True)
            (licenses / 'LICENSE').write_text('dependency license')
            for name in ('dish.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'BUILD.json'):
                (payload / name).write_text(name)
            outputs = []
            for name in ('first', 'second'):
                source = root / name / 'Dish.wxs'
                subprocess.run(['pwsh', '-NoProfile', '-NonInteractive', '-File',
                                str(scripts / 'package-msi.ps1'), '-Version', '1.2.3.4',
                                '-OutputPath', str(root / name / 'Dish.msi'),
                                '-SourcePath', str(source), '-GenerateOnly'],
                               check=True, capture_output=True, text=True)
                self.assertFalse((source.parent / 'Dish.msi').exists())
                self.assertTrue((source.parent / 'license.rtf').is_file())
                outputs.append(source.read_bytes())
            self.assertEqual(outputs[0], outputs[1])
            ns = {'w': 'http://wixtoolset.org/schemas/v4/wxs'}
            document = ET.fromstring(outputs[0])
            files = document.findall('.//w:File/..', ns)
            self.assertEqual(len(files), 6)
            namespace = uuid.UUID('E2829A7B-5D29-4FCC-989B-73EB8D15336E')
            for component in files:
                registry = component.find('w:RegistryValue', ns)
                self.assertEqual(registry.attrib['Root'], 'HKCU')
                self.assertEqual(registry.attrib['KeyPath'], 'yes')
                file = component.find('w:File', ns)
                path = Path(file.attrib['Source'])
                logical = path.relative_to(payload).as_posix() if path.is_relative_to(payload) else path.name
                expected = uuid.uuid5(namespace, 'file/' + logical.lower())
                self.assertEqual(component.attrib['Guid'], str(expected).upper())

    @unittest.skipUnless(shutil.which('pwsh'), 'PowerShell is required to execute the UUID v5 helper')
    def test_msi_component_guids_match_uuid5_and_normalize_paths(self):
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        # Execute the production helper, independently of WiX or Windows.
        helper = script[script.index('$ComponentGuidNamespace ='):script.index('$componentRefs =')]
        names = ['file/dish.exe', 'file/licenses/example/LICENSE',
                 r'FILE\LICENSES\EXAMPLE\license', 'directory/install',
                 'shortcut/start-menu', 'shortcut/desktop', 'file/dish.exe']
        invocation = '\n'.join("Get-ComponentGuid '" + name + "'" for name in names)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'guid-vectors.ps1'
            path.write_text("$ErrorActionPreference = 'Stop'\n" + helper + invocation, encoding='utf-8')
            output = subprocess.check_output(['pwsh', '-NoProfile', '-NonInteractive', '-File', str(path)],
                                             text=True)
        namespace = uuid.UUID('E2829A7B-5D29-4FCC-989B-73EB8D15336E')
        expected = [str(uuid.uuid5(namespace, name.replace('\\', '/').lower())).upper() for name in names]
        self.assertEqual(output.splitlines(), expected)
        self.assertEqual(expected[1], expected[2])
        self.assertEqual(expected[0], expected[-1])

    def test_msi_installs_the_nsis_payload_per_user_with_optional_desktop_shortcut(self):
        script = (ROOT / "scripts/package-msi.ps1").read_text()
        for name in ('dish.exe', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'BUILD.json',
                     'install-pi-windows.ps1', 'licenses'):
            self.assertIn("'" + name + "'", script)
        self.assertIn("dist/windows-payload", script)
        self.assertIn('Scope="perUser"', script)
        self.assertIn('-arch x64', script)
        self.assertIn('<StandardDirectory Id="LocalAppDataFolder">', script)
        self.assertIn('<Directory Id="UserProgramsFolder" Name="Programs">', script)
        self.assertIn('<Directory Id="INSTALLFOLDER" Name="Dish">', script)
        self.assertIn('<ComponentRef Id="StartMenuShortcut" />', script)
        self.assertRegex(script, r'<Feature Id="DesktopShortcutFeature"[^>]+Level="2"')
        self.assertIn('<Property Id="INSTALLLEVEL" Value="1" />', script)
        self.assertIn('<ui:WixUI Id="WixUI_FeatureTree" />', script)
        self.assertIn('Id="ARPPRODUCTICON" Value="DishIcon"', script)
        self.assertIn('Root="HKCU"', script)
        self.assertIn('<RemoveFolder', script)

    def test_msi_is_signed_before_checksum_and_uploaded_with_nsis(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text()
        stages = ['python3 scripts/package-windows.py', 'name: Install WiX',
                  'name: Build the MSI installer', 'name: Sign and verify the MSI installer',
                  'name: Checksum the final MSI installer', 'name: Save the Windows installer']
        offsets = [workflow.index(stage) for stage in stages]
        self.assertEqual(offsets, sorted(offsets))
        sign = workflow.split('name: Sign and verify the MSI installer', 1)[1].split('      - name:', 1)[0]
        for secret in ('WINDOWS_CERT_PFX_BASE64', 'WINDOWS_CERT_PASSWORD'):
            self.assertIn('${{ secrets.' + secret + ' }}', sign)
        self.assertIn('sign-windows.ps1 -Path "dist/dish-$env:RELEASE_LABEL-windows-x86_64.msi"', sign)
        checksum = workflow.split('name: Checksum the final MSI installer', 1)[1].split('      - name:', 1)[0]
        self.assertIn('Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256', checksum)
        self.assertIn('"$hash  $($file.Name)`n"', checksum)
        artifact = workflow.split('name: Save the Windows installer', 1)[1].split('  publish:', 1)[0]
        for suffix in ('-setup.exe', '-setup.exe.sha256', '.msi', '.msi.sha256'):
            self.assertIn('dist/*-windows-x86_64' + suffix, artifact)
        self.assertIn('if-no-files-found: error', artifact)
        self.assertNotIn('continue-on-error', workflow)
        self.assertNotIn('|| true', workflow)

    def test_msi_and_checksum_are_required_for_release_publication(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text()
        publish = workflow.split('  publish:\n', 1)[1]
        self.assertIn('needs: windows', publish)
        self.assertIn('name: dish-windows-installer', publish)
        self.assertIn('msi="dish-$version-windows-x86_64.msi"', publish)
        self.assertIn('test -s "dist-win/$msi"', publish)
        self.assertIn('sha256sum -c "$msi.sha256"', publish)
        self.assertIn("printf 'MSI_ASSET=%s\\n'", publish)
        upload = next(line for line in publish.splitlines() if 'gh release upload' in line)
        for asset in ('$ASSET', '$ASSET.sha256', '$MSI_ASSET', '$MSI_ASSET.sha256'):
            self.assertIn('"dist-win/' + asset + '"', upload)

    def test_wix_and_ui_extension_are_pinned_and_fail_closed(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text()
        install = workflow.split('name: Install WiX', 1)[1].split('      - name:', 1)[0]
        self.assertIn('dotnet tool install --global wix --version 4.0.6', install)
        self.assertIn('wix --version', install)
        self.assertIn('wix extension add --global WixToolset.UI.wixext/4.0.6', install)
        self.assertEqual(install.count('if ($LASTEXITCODE -ne 0) { throw'), 3)
        script = (ROOT / 'scripts/package-msi.ps1').read_text()
        self.assertIn('-ext WixToolset.UI.wixext/4.0.6', script)
        self.assertIn('if ($LASTEXITCODE -ne 0) { throw', script)

    def test_installer_is_published_only_for_a_release(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text(encoding="utf-8")
        # O gatilho é publicar o Release; um push (mesmo de tag) não publica.
        self.assertIn("  release:\n    types: [published]", workflow)
        self.assertIn("if: github.event_name == 'release'", workflow)
        self.assertIn("gh release upload", workflow)
        # Asset publicado não é substituído (imutabilidade).
        for line in workflow.splitlines():
            if "gh release upload" in line:
                self.assertNotIn("--clobber", line)

    def test_write_permission_belongs_only_to_the_upload_job(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text(encoding="utf-8")
        top, jobs = workflow.split("jobs:", 1)
        build, publish = jobs.split("  publish:\n", 1)
        self.assertIn("permissions:\n  contents: read", top)
        self.assertNotIn("contents: write", build)
        self.assertIn("contents: write", publish)

    def test_release_refuses_an_installer_that_does_not_match_the_tag(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text(encoding="utf-8")
        publish = workflow.split("  publish:\n", 1)[1]
        self.assertIn("${version%-windows-x86_64-setup.exe}", publish)
        self.assertIn("GITHUB_REF_NAME", publish)
        self.assertIn("sha256sum -c", publish)
        # O artifact precisa carregar o checksum junto do instalador.
        self.assertIn("dist/*-windows-x86_64-setup.exe.sha256", workflow)

    def test_published_crate_without_license_files_uses_standard_texts(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "licenses/standard").mkdir(parents=True)
            shutil.copyfile(ROOT / "licenses/standard/MIT.txt", root / "licenses/standard/MIT.txt")
            (root / "LICENSE").write_text("apache", encoding="utf-8")
            (root / "licenses/overrides.json").write_text("[]", encoding="utf-8")
            crate = root / "registry/src/index/plain-1.0.0"
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").touch()
            archive = root / "registry/cache/index/plain-1.0.0.crate"
            archive.parent.mkdir(parents=True)
            archive.write_bytes(b"published crate")
            lock = {"package": [{"name": "plain", "version": "1.0.0", "source": "registry+test",
                                 "checksum": hashlib.sha256(archive.read_bytes()).hexdigest()}]}
            (root / "Cargo.lock").write_text("package = []", encoding="utf-8")
            metadata = {"resolve": {"root": "dish"}, "packages": [
                {"id": "plain", "name": "plain", "version": "1.0.0", "license": "MIT OR Apache-2.0",
                 "source": "registry+test", "manifest_path": str(crate / "Cargo.toml")}]}
            import license_bundle
            with patch.object(license_bundle, "tomllib") as tomllib_mock:
                tomllib_mock.loads.return_value = lock
                count = license_bundle.collect_licenses(metadata, root / "out", root)
            self.assertEqual(count, 1)
            files = sorted(p.name for p in (root / "out/dependencies/plain-1.0.0").iterdir())
            self.assertEqual(files, ["0-MIT.txt", "1-LICENSE"])
            record = json.loads((root / "out/dependencies.json").read_text(encoding="utf-8"))[0]
            self.assertIn("standard_texts", record["override"])
            # A crate completa acompanha os textos padrão, preservando os avisos.
            self.assertTrue((root / "out/sources/plain-1.0.0.crate").is_file())

    def test_unknown_license_without_files_still_fails_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "licenses").mkdir(parents=True)
            (root / "licenses/overrides.json").write_text("[]", encoding="utf-8")
            (root / "Cargo.lock").write_text("package = []", encoding="utf-8")
            crate = root / "registry/src/index/weird-1.0.0"
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").touch()
            metadata = {"resolve": {"root": "dish"}, "packages": [
                {"id": "weird", "name": "weird", "version": "1.0.0", "license": "LicenseRef-Proprietary",
                 "source": "registry+test", "manifest_path": str(crate / "Cargo.toml")}]}
            import license_bundle
            with self.assertRaisesRegex(RuntimeError, "Missing license texts"):
                license_bundle.collect_licenses(metadata, root / "out", root)

    def test_installer_script_keeps_the_utf8_bom(self):
        # Sem BOM o NSIS interpreta o script como ANSI e os acentos das telas
        # saem corrompidos ("Ã¡rea de trabalho").
        data = (ROOT / "scripts/windows-installer.nsi").read_bytes()
        self.assertTrue(data.startswith(b"\xef\xbb\xbf"), "windows-installer.nsi precisa do BOM UTF-8")
        # BOM duplicado quebra o makensis ("Invalid command") — já aconteceu.
        self.assertFalse(data.startswith(b"\xef\xbb\xbf\xef\xbb\xbf"))
        self.assertFalse(data.decode("utf-8-sig").startswith("\ufeff"))

    def test_installer_requires_pi_opt_in_and_holds_dish_back(self):
        nsi = (ROOT / "scripts/windows-installer.nsi").read_text(encoding="utf-8")
        # Download requires opt-in; execution requires a second confirmation.
        self.assertIn("${NSD_Uncheck} $PiCheckbox", nsi)
        self.assertNotIn("${NSD_Check} $PiCheckbox", nsi)
        self.assertNotIn("ExecutionPolicy", nsi)
        self.assertNotIn("| iex", nsi)
        self.assertIn('File "${SRCDIR}\\scripts\\install-pi-windows.ps1"', nsi)
        self.assertIn('-NoProfile -File "$INSTDIR\\install-pi-windows.ps1"', nsi)
        self.assertIn("ExecShell \"open\" \"$INSTDIR\\${APP_EXE}\"", nsi)
        # O "Executar o Dish" fica desmarcado, mas nunca desabilitado.
        self.assertIn("${NSD_Uncheck} $DishCheckbox", nsi)
        self.assertNotIn("EnableWindow $DishCheckbox", nsi)

    def test_windows_download_is_reviewed_before_explicit_execution(self):
        script = (ROOT / "scripts/install-pi-windows.ps1").read_text()
        stages = ["Invoke-WebRequest", "Get-FileHash", "Get-AuthenticodeSignature",
                  "Start-Process", "-cne 'INSTALAR'", "-ne $hash",
                  "& powershell.exe -NoProfile -File $script"]
        offsets = [script.index(stage) for stage in stages]
        self.assertEqual(offsets, sorted(offsets))
        self.assertIn("-Wait", script)
        for unsafe in ["ExecutionPolicy", "Invoke-Expression", "| iex", "Unblock-File"]:
            self.assertNotIn(unsafe, script)
        installation = (ROOT / "src/installation.rs").read_text()
        self.assertIn('include_str!("../scripts/install-pi-windows.ps1")', installation)

    def test_signing_happens_before_packaging_and_before_upload(self):
        workflow = (ROOT / ".github/workflows/windows.yml").read_text()
        stages = ["sign-windows.ps1 -Path 'target/release/dish.exe'",
                  "python3 scripts/package-windows.py",
                  "sign-windows.ps1 -Path 'dist/*-windows-x86_64-setup.exe'",
                  "name: Save Windows executable"]
        offsets = [workflow.index(stage) for stage in stages]
        self.assertEqual(offsets, sorted(offsets))
        script = (ROOT / "scripts/sign-windows.ps1").read_text()
        self.assertIn("verify /pa /v", script)
        self.assertIn("/fd SHA256 /tr", script)
        self.assertIn("/td SHA256", script)
        self.assertLess(script.index("verify /pa /v"), script.index("Get-FileHash"))
        self.assertIn("Signing secrets absent", script)
        self.assertIn("finally", script)
        self.assertIn("-DeleteKey", script)

    def test_shared_bundle_module_is_used_by_both_packagers(self):
        for script in ["package-linux.py", "package-windows.py"]:
            text = (ROOT / "scripts" / script).read_text()
            self.assertIn("license_bundle", text, script)
        self.assertTrue((ROOT / "scripts/license_bundle.py").is_file())


class _DownloadResponse:
    """Resposta mínima de `urlopen`; nenhum destes testes toca a rede."""

    def __init__(self, body: bytes):
        self.body = body

    def read(self) -> bytes:
        return self.body

    def __enter__(self):
        return self

    def __exit__(self, *exc_info):
        return False


def http_error(url: str, code: int, headers: dict | None = None) -> urllib.error.HTTPError:
    # BytesIO evita o arquivo temporário que o HTTPError criaria com fp=None.
    return urllib.error.HTTPError(url, code, f"HTTP {code}", headers or {}, io.BytesIO(b""))


class LicenseDownloadTests(unittest.TestCase):
    URL = "https://raw.githubusercontent.com/rust-lang/rust/" + "a" * 40 + "/LICENSE-MIT"

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.cache = Path(temp.name)

    def test_transient_429_is_retried_until_success(self):
        sleeps = []
        responses = [http_error(self.URL, 429, {"Retry-After": "0"}), _DownloadResponse(b"license text")]
        with patch("urllib.request.urlopen", side_effect=responses) as urlopen:
            data = license_bundle.download(self.URL, cache_dir=self.cache, sleep=sleeps.append)
        self.assertEqual(data, b"license text")
        self.assertEqual(urlopen.call_count, 2)
        # Retry-After: 0 manda esperar zero; o teste não pode dormir de verdade.
        self.assertEqual(sleeps, [])

    def test_client_errors_fail_on_the_first_attempt(self):
        for code in (403, 404):
            with self.subTest(code=code):
                with patch("urllib.request.urlopen", side_effect=http_error(self.URL, code)) as urlopen:
                    with self.assertRaises(urllib.error.HTTPError) as caught:
                        license_bundle.download(self.URL, cache_dir=self.cache,
                                                sleep=lambda seconds: self.fail(f"slept {seconds}s"))
                self.assertEqual(caught.exception.code, code)
                self.assertEqual(urlopen.call_count, 1)

    def test_exhausted_retries_raise_runtime_error_with_url(self):
        sleeps = []
        with patch("urllib.request.urlopen", side_effect=http_error(self.URL, 503)) as urlopen:
            with self.assertRaises(RuntimeError) as caught:
                license_bundle.download(self.URL, cache_dir=self.cache, attempts=3,
                                        base_delay=0, sleep=sleeps.append)
        self.assertIn(self.URL, str(caught.exception))
        self.assertIn("3 attempts", str(caught.exception))
        self.assertIn("HTTP 503", str(caught.exception))
        self.assertEqual(urlopen.call_count, 3)
        self.assertEqual(sleeps, [])

    def test_retry_after_is_honoured_and_capped(self):
        cases = [("7", 7.0), ("3600", license_bundle.RETRY_AFTER_CAP)]
        for header, expected in cases:
            with self.subTest(header=header):
                sleeps = []
                url = f"{self.URL}?retry-after={header}"
                responses = [http_error(url, 429, {"Retry-After": header}), _DownloadResponse(b"ok")]
                with patch("urllib.request.urlopen", side_effect=responses):
                    license_bundle.download(url, cache_dir=self.cache, sleep=sleeps.append)
                self.assertEqual(sleeps, [expected])

    def test_missing_retry_after_uses_bounded_exponential_backoff(self):
        sleeps = []
        responses = [http_error(self.URL, 502), http_error(self.URL, 503), _DownloadResponse(b"ok")]
        with patch("urllib.request.urlopen", side_effect=responses):
            data = license_bundle.download(self.URL, cache_dir=self.cache, base_delay=0.25,
                                           sleep=sleeps.append)
        self.assertEqual(data, b"ok")
        self.assertEqual(len(sleeps), 2)
        self.assertGreaterEqual(sleeps[0], 0.25)
        self.assertLessEqual(sleeps[0], 0.375)
        self.assertGreaterEqual(sleeps[1], 0.5)
        self.assertLessEqual(sleeps[1], 0.75)

    def test_url_and_timeout_errors_are_retried(self):
        responses = [urllib.error.URLError("connection reset"), TimeoutError("timed out"),
                     _DownloadResponse(b"recovered")]
        with patch("urllib.request.urlopen", side_effect=responses) as urlopen:
            data = license_bundle.download(self.URL, cache_dir=self.cache, base_delay=0,
                                           sleep=lambda seconds: None)
        self.assertEqual(data, b"recovered")
        self.assertEqual(urlopen.call_count, 3)

    def test_cache_avoids_a_second_network_call(self):
        with patch("urllib.request.urlopen", return_value=_DownloadResponse(b"cached text")) as urlopen:
            first = license_bundle.download(self.URL, cache_dir=self.cache)
            second = license_bundle.download(self.URL, cache_dir=self.cache)
        self.assertEqual((first, second), (b"cached text", b"cached text"))
        self.assertEqual(urlopen.call_count, 1)
        hashed = hashlib.sha256(self.URL.encode("utf-8")).hexdigest()
        self.assertEqual((self.cache / f"{hashed}.bin").read_bytes(), b"cached text")

    def test_empty_cache_entry_is_not_treated_as_valid(self):
        hashed = hashlib.sha256(self.URL.encode("utf-8")).hexdigest()
        cached = self.cache / f"{hashed}.bin"
        cached.write_bytes(b"")
        with patch("urllib.request.urlopen", return_value=_DownloadResponse(b"fresh text")) as urlopen:
            data = license_bundle.download(self.URL, cache_dir=self.cache)
        self.assertEqual(data, b"fresh text")
        self.assertEqual(urlopen.call_count, 1)
        self.assertEqual(cached.read_bytes(), b"fresh text")

    def test_toolchain_licenses_records_the_downloaded_bytes(self):
        commit = "b" * 40
        payloads = {"LICENSE-APACHE": b"apache text", "LICENSE-MIT": b"mit text",
                    "COPYRIGHT": b"copyright text"}
        sysroot = self.cache / "sysroot"
        documentation = sysroot / "share/doc/rust"
        (documentation / "licenses").mkdir(parents=True)
        (documentation / "COPYRIGHT-library.html").write_text("library notices", encoding="utf-8")
        (documentation / "licenses/MIT.txt").write_text("mit", encoding="utf-8")

        def fake_check_output(args, **kwargs):
            return f"release: 1.90.0\ncommit-hash: {commit}\n" if args[1] == "-vV" else str(sysroot)

        def fake_download(url, **kwargs):
            return payloads[url.rsplit("/", 1)[-1]]

        destination = self.cache / "out"
        with patch.object(license_bundle.subprocess, "check_output", side_effect=fake_check_output), \
                patch.object(license_bundle, "download", side_effect=fake_download):
            license_bundle.toolchain_licenses(destination)
        provenance = json.loads((destination / "rust-toolchain/provenance.json").read_text(encoding="utf-8"))
        self.assertEqual(provenance["rustc"], "1.90.0")
        downloaded = [entry for entry in provenance["files"] if entry["file"] in payloads]
        self.assertEqual(len(downloaded), 3)
        for entry in downloaded:
            self.assertIn(commit, entry["url"])
            self.assertEqual(entry["sha256"], hashlib.sha256(payloads[entry["file"]]).hexdigest())


@unittest.skipUnless(shutil.which("dpkg-deb") and shutil.which("dpkg-query") and shutil.which("readelf"),
                     "Debian packaging tools (dpkg-deb, dpkg-query, readelf)")
class IconAssetTests(unittest.TestCase):
    """O prato é um só: cada formato de ícone vem da mesma renderização."""

    def ico_entries(self, data: bytes) -> list[tuple[int, bytes]]:
        reserved, kind, count = struct.unpack("<HHH", data[:6])
        self.assertEqual((reserved, kind), (0, 1))
        entries = []
        end = 0
        for index in range(count):
            width, height, _, _, _, depth, size, offset = struct.unpack(
                "<BBBBHHII", data[6 + 16 * index:22 + 16 * index])
            self.assertEqual(depth, 32)
            payload = data[offset:offset + size]
            self.assertEqual(len(payload), size)
            end = max(end, offset + size)
            entries.append((width or 256, payload))
        # Nenhum byte sobrando depois do último payload.
        self.assertEqual(end, len(data))
        return entries

    def test_ico_covers_the_shell_sizes_with_png_payloads(self):
        data = (ROOT / "assets/dish-icon.ico").read_bytes()
        entries = self.ico_entries(data)
        self.assertEqual([size for size, _ in entries], [16, 32, 48, 64, 128, 256])
        for size, payload in entries:
            self.assertEqual(payload[:8], b"\x89PNG\r\n\x1a\n", size)

    def test_icns_has_the_modern_element_set(self):
        data = (ROOT / "assets/dish-icon.icns").read_bytes()
        self.assertEqual(data[:4], b"icns")
        self.assertEqual(struct.unpack(">I", data[4:8])[0], len(data))
        elements = {}
        offset = 8
        while offset < len(data):
            kind = data[offset:offset + 4].decode("ascii")
            size = struct.unpack(">I", data[offset + 4:offset + 8])[0]
            self.assertEqual(data[offset + 8:offset + 16], b"\x89PNG\r\n\x1a\n", kind)
            elements[kind] = size
            offset += size
        self.assertEqual(offset, len(data))
        self.assertEqual(set(elements), {
            "ic07", "ic08", "ic09", "ic10", "ic11", "ic12", "ic13", "ic14"})

    def test_build_script_owns_both_formats(self):
        script = (ROOT / "assets/build-icon.py").read_text(encoding="utf-8")
        self.assertIn('write_ico(here / "dish-icon.ico"', script)
        self.assertIn('write_icns(', script)
        self.assertIn('here / "dish-icon.icns"', script)


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
            self.assertEqual(built.name, "dish-pi_0.1.0-alpha.1_amd64.deb")
            self.assertEqual(built.parent, output)
            self.assertTrue(Path(str(built) + ".sha256").is_file())
            info = subprocess.check_output(["dpkg-deb", "--info", str(built)], text=True)
            self.assertIn("Version: 0.1.0~alpha.1", info)
            self.assertIn("Package: dish-pi", info)
            listing = subprocess.check_output(["dpkg-deb", "--contents", str(built)], text=True)
            installed = [line.rsplit(" ", 1)[-1].removeprefix("./") for line in listing.splitlines()]
            for member in ["usr/bin/dish-pi", "usr/share/applications/dish-pi.desktop",
                           "usr/share/icons/hicolor/512x512/apps/dish-pi.png", "usr/share/doc/dish-pi/copyright",
                           "usr/share/doc/dish-pi/changelog", "usr/share/doc/dish-pi/licenses/dependencies/entry.txt"]:
                self.assertIn(member, installed)
            # A colisão com o pacote `dish` do Debian: nem o binário, nem o
            # atalho, nem a documentação disputam os caminhos dele.
            self.assertNotIn("usr/bin/dish", installed)
            self.assertNotIn("usr/share/applications/dish.desktop", installed)
            self.assertNotIn("usr/share/doc/dish/copyright", installed)
            # A sidecar checksum that no longer matches must stop the build.
            Path(str(archive) + ".sha256").write_text("0" * 64 + f"  {archive.name}\n")
            with self.assertRaisesRegex(RuntimeError, "Checksum mismatch"):
                deb.build(archive, output)

    def test_package_does_not_dispute_the_debian_dish_name(self):
        """O Debian também tem um pacote `dish` (diligence/distributed shell) e
        as versões dele são maiores; o nosso não pode declarar conflito nem
        instalar nos mesmos caminhos, senão o apt troca um pelo outro."""
        control = deb.control_text("0.1.0~alpha.1", "2.39", ["libc6"], 0)
        self.assertIn("Package: dish-pi", control)
        # Nada de Conflicts/Replaces/Breaks/Provides: eles removeriam o pacote
        # legítimo do Debian junto.
        for field in ("Conflicts:", "Replaces:", "Breaks:", "Provides:"):
            self.assertNotIn(field, control)
        entry = deb.desktop_entry()
        self.assertIn("Exec=dish-pi", entry)
        self.assertIn("Icon=dish-pi", entry)
        # O app_id da janela continua `dish`; sem isso o GNOME não casa a
        # janela com o atalho.
        self.assertIn("StartupWMClass=dish", entry)

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
