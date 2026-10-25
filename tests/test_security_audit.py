"""Synthetic scanner regressions; never include live credentials."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path.insert(0, str(SCRIPTS))
import security_audit as audit


class SecurityAuditTests(unittest.TestCase):
    def test_cvss_severity_and_unknown_vectors(self):
        self.assertEqual(audit.cvss_severity('CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H'), 'critical')
        self.assertEqual(audit.cvss_severity('CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:H'), 'high')
        self.assertEqual(audit.cvss_severity(None), 'unknown')
        self.assertEqual(audit.cvss_severity('CVSS:4.0/AV:N'), 'unknown')

    def test_common_secret_formats_and_redaction(self):
        samples = [b'AKIA' + b'A' * 16, b'aws_secret_access_key=' + b'A' * 40,
                   b'ghp_' + b'B' * 36,
                   b'github_pat_' + b'C' * 82, b'sk-proj-' + b'D' * 40,
                   b'sk-ant-api03-' + b'E' * 40,
                   b'-----BEGIN ' + b'OPENSSH PRIVATE KEY-----',
                   b'https://' + b'user:synthetic-password@example.invalid']
        for value in samples:
            found = audit.matches(value)
            self.assertTrue(found)
            self.assertNotIn(value.decode(), repr(found))

    def test_binary_is_excluded(self):
        self.assertEqual(audit.matches(b'\0ghp_' + b'B' * 36), [])

    def test_license_choices_and_obligations(self):
        self.assertTrue(audit.license_allowed('Apache-2.0 OR GPL-2.0-only', 'self_cell'))
        self.assertTrue(audit.license_allowed('(MIT OR Apache-2.0) AND Unicode-3.0', 'unicode-ident'))
        self.assertTrue(audit.license_allowed('MPL-2.0', 'option-ext'))
        for expression in ['', 'GPL-2.0-only', 'MIT AND LicenseRef-Unknown', 'MIT OR', '(MIT', 'MIT garbage']:
            self.assertFalse(audit.license_allowed(expression, 'example'))

    def test_missing_auditor_is_incomplete_not_clean(self):
        with patch.object(audit.shutil, 'which', return_value=None), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(audit.dependencies(), 2)

    def test_local_auditor_uses_offline_database_and_preserves_status(self):
        for returncode, warnings, expected in [
            (0, {}, 0), (0, {"unsound": [{"package": {"name": "synthetic"}}]}, 1),
            (2, {}, 2), (2, {"unsound": [{"package": {"name": "synthetic"}}]}, 3),
        ]:
            with self.subTest(returncode=returncode, expected=expected):
                report = {"vulnerabilities": {"list": []}, "warnings": warnings}
                result = subprocess.CompletedProcess([], returncode, stdout=json.dumps(report).encode())
                with patch.dict(audit.os.environ, {"CARGO_AUDIT": "/local/cargo-audit",
                                                   "CARGO_AUDIT_DB": "/local/advisory-db"}), \
                     patch.object(audit.subprocess, 'run', return_value=result) as run, \
                     contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(audit.dependencies(), expected)
                self.assertEqual(run.call_args.args[0], [
                    "/local/cargo-audit", "audit", "--file", "Cargo.lock", "--json",
                    "--no-fetch", "--no-yanked", "--db", "/local/advisory-db"])

    def test_coordinator_installation_is_in_detection_path(self):
        with patch.dict(audit.os.environ, {}, clear=True), \
             patch.object(audit.shutil, 'which', return_value=None) as which, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(audit.dependencies(), 2)
        self.assertIn('/tmp/dish-security-tools/bin', which.call_args.kwargs['path'].split(audit.os.pathsep))

    def test_deleted_secret_on_other_branch_is_detected_without_disclosure(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            def git(*args):
                return subprocess.check_output(['git', '-C', tmp, *args], stderr=subprocess.DEVNULL)
            git('init', '-b', 'main')
            git('config', 'user.name', 'Synthetic Test')
            git('config', 'user.email', 'test@example.invalid')
            (root / 'clean.txt').write_text('clean\n')
            git('add', '.')
            git('commit', '-m', 'clean')
            git('checkout', '-b', 'historical')
            secret = 'ghp_' + 'Z' * 36
            (root / 'removed.txt').write_text(secret)
            git('add', '.')
            git('commit', '-m', 'synthetic historical fixture')
            git('rm', 'removed.txt')
            git('commit', '-m', 'delete fixture')
            git('checkout', 'main')
            output = io.StringIO()
            with patch.object(audit, 'ROOT', root), contextlib.redirect_stdout(output):
                self.assertEqual(audit.secrets(), 1)
            self.assertIn('github-token', output.getvalue())
            self.assertNotIn(secret, output.getvalue())

    def test_tool_failure_is_not_zero_vulnerabilities(self):
        result = subprocess.CompletedProcess([], 2, stdout=b'not json', stderr=b'offline')
        with patch.object(audit.shutil, 'which', return_value='/synthetic/cargo-audit'), \
             patch.object(audit.subprocess, 'run', return_value=result), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(audit.dependencies(), 2)
        self.assertIn('N/A', output.getvalue())


if __name__ == '__main__':
    unittest.main()
