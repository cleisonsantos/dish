"""Compile the actual release identity module with and without the build label."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
WIN_SPEC = importlib.util.spec_from_file_location(
    "package_windows_release", ROOT / "scripts/package-windows.py")
windows = importlib.util.module_from_spec(WIN_SPEC)
WIN_SPEC.loader.exec_module(windows)


class ReleaseVersionTests(unittest.TestCase):
    def test_compiled_build_labels_and_local_fallback(self):
        with tempfile.TemporaryDirectory(prefix="dish-version-") as temporary:
            directory = Path(temporary)
            source = directory / "main.rs"
            source.write_text(
                '#[path = ' + '"' + str(ROOT / 'src/version.rs') + '"] mod version;\n'
                'fn main() { println!("{}|{}|{}", version::release_version(), '
                'version::is_local_build(), version::display_version()); }\n')
            for label, expected in [(None, "0.1.0|true|0.1.0 (build local)"),
                                    ("0.1.0-alpha.6", "0.1.0-alpha.6|false|0.1.0-alpha.6"),
                                    ("0.1.0-local.1", "0.1.0-local.1|true|0.1.0-local.1 (build local)")]:
                with self.subTest(label=label):
                    env = os.environ.copy()
                    env["CARGO_PKG_VERSION"] = "0.1.0"
                    env.pop("DISH_RELEASE_VERSION", None)
                    if label is not None:
                        env["DISH_RELEASE_VERSION"] = label
                    binary = directory / "version-test"
                    subprocess.run(["rustc", "--edition=2021", str(source), "-o", str(binary)],
                                   env=env, check=True, capture_output=True, text=True)
                    self.assertEqual(subprocess.check_output([str(binary)], text=True).strip(), expected)


class WindowsReleaseLabelTests(unittest.TestCase):
    def test_repository_ci_label_keeps_the_prerelease_suffix(self):
        self.assertEqual(windows.release_version(None), "0.1.0-beta.1")

    def test_dish_version_environment_spelling(self):
        with tempfile.TemporaryDirectory(prefix="dish-ci-") as temporary:
            root = Path(temporary)
            ci = root / ".github/workflows/ci.yml"
            ci.parent.mkdir(parents=True)
            ci.write_text("jobs:\n  package:\n    env:\n      DISH_VERSION: 0.1.0-beta.7\n",
                          encoding="utf-8")
            with patch.object(windows, "ROOT", root):
                self.assertEqual(windows.release_version(None), "0.1.0-beta.7")

    def test_version_command_spelling(self):
        with tempfile.TemporaryDirectory(prefix="dish-ci-") as temporary:
            root = Path(temporary)
            ci = root / ".github/workflows/ci.yml"
            ci.parent.mkdir(parents=True)
            ci.write_text("run: python3 scripts/package-linux.py --version 0.1.0-beta.8\n",
                          encoding="utf-8")
            with patch.object(windows, "ROOT", root):
                self.assertEqual(windows.release_version(None), "0.1.0-beta.8")

    def test_ci_without_a_release_label_is_an_error(self):
        with tempfile.TemporaryDirectory(prefix="dish-ci-") as temporary:
            root = Path(temporary)
            ci = root / ".github/workflows/ci.yml"
            ci.parent.mkdir(parents=True)
            ci.write_text("name: CI\njobs: {}\n", encoding="utf-8")
            with patch.object(windows, "ROOT", root):
                with self.assertRaisesRegex(RuntimeError, "defines no release label"):
                    windows.release_version(None)

    def test_resolved_label_does_not_collapse_to_the_base_version(self):
        label = windows.release_version(None)
        self.assertNotEqual(windows.numeric_version(label), "0.1.0.0")


if __name__ == "__main__":
    unittest.main()
