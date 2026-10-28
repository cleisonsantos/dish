import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('rpm_package', Path(__file__).resolve().parents[1] / 'scripts/package-rpm.py')
rpm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rpm)


class RpmPackageTests(unittest.TestCase):
    def test_label_and_version(self):
        self.assertEqual(rpm.artifact_name('0.1.0-beta.1'), 'dish-0.1.0-beta.1-1.x86_64.rpm')
        self.assertEqual(rpm.rpm_version('0.1.0-beta.1'), '0.1.0~beta.1')
        self.assertEqual(rpm.rpm_version('0.1.0'), '0.1.0')
        with self.assertRaises(ValueError):
            rpm.rpm_version('0.1.0-')

    def test_spec_lists_payload_and_glibc(self):
        spec_text = rpm.spec_text('0.1.0~beta.1', '2.39')
        self.assertIn('Version: 0.1.0~beta.1\n', spec_text)
        self.assertIn('Requires: glibc >= 2.39\n', spec_text)
        files = spec_text.split('%files\n')[1].splitlines()
        self.assertEqual(files, ['/usr/bin/dish-pi', '/usr/share/applications/dish-pi.desktop',
                                 '/usr/share/icons/hicolor/512x512/apps/dish-pi.png', '/usr/share/doc/dish-pi'])


if __name__ == '__main__':
    unittest.main()
