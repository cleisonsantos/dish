"""Compile the actual release identity module with and without the build label."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


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


if __name__ == "__main__":
    unittest.main()
