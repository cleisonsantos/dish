import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('appimage_package', Path(__file__).resolve().parents[1] / 'scripts/package-appimage.py')
app = importlib.util.module_from_spec(spec)
spec.loader.exec_module(app)


class AppImagePackageTests(unittest.TestCase):
    def test_name(self):
        self.assertEqual(app.artifact_name('0.1.0-beta.1'), 'dish-0.1.0-beta.1-x86_64.AppImage')

    def test_staging(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = root / 'package'
            (package / 'bin').mkdir(parents=True)
            (package / 'bin/dish').write_bytes(b'binary')
            (package / 'share/icons').mkdir(parents=True)
            (package / 'share/icons/dish.png').write_bytes(b'icon')
            (package / 'share/licenses').mkdir()
            (package / 'share/licenses/LICENSE').write_text('license')
            appdir = root / 'AppDir'
            app.stage_tree(package, appdir)
            self.assertEqual((appdir / 'usr/bin/dish').read_bytes(), b'binary')
            self.assertTrue((appdir / 'AppRun').stat().st_mode & 0o111)
            self.assertIn('/usr/bin/dish" "$@"', (appdir / 'AppRun').read_text())
            desktop = (appdir / 'dish.desktop').read_text()
            self.assertIn('\nExec=dish\n', desktop)
            self.assertIn('\nIcon=dish\n', desktop)
            self.assertIn('\nCategories=Development;\n', desktop)
            self.assertEqual((appdir / 'dish.png').read_bytes(), b'icon')
            self.assertTrue((appdir / 'usr/share/doc/dish/licenses/LICENSE').is_file())


if __name__ == '__main__':
    unittest.main()
