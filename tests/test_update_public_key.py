"""The shipped update key must be a real minisign key, not a placeholder."""
import base64
import re
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
UPDATE_RS = ROOT / "src/update.rs"
PLACEHOLDER = "PRODUCTION_KEY_NOT_PROVISIONED"
KEY_PATTERN = re.compile(r'pub const UPDATE_PUBLIC_KEY: &str = "([^"]+)";')


def update_public_key():
    match = KEY_PATTERN.search(UPDATE_RS.read_text(encoding="utf-8"))
    if match is None:
        raise AssertionError("UPDATE_PUBLIC_KEY declaration not found")
    return match.group(1)


class UpdatePublicKeyTests(unittest.TestCase):
    def test_placeholder_is_gone(self):
        self.assertNotEqual(update_public_key(), PLACEHOLDER)
        self.assertNotIn(PLACEHOLDER, UPDATE_RS.read_text(encoding="utf-8"))

    def test_decodes_as_a_minisign_public_key(self):
        encoded = update_public_key()
        # The constant is the bare base64 line, never the public-key comment line.
        self.assertNotIn("\n", encoded)
        self.assertNotIn("untrusted comment", encoded)
        raw = base64.b64decode(encoded, validate=True)
        self.assertEqual(len(raw), 42, "minisign keys are 2 + 8 + 32 bytes")
        self.assertIn(raw[:2], (b"Ed", b"ED"), "Ed25519 minisign signature algorithm")
        self.assertNotEqual(raw[2:10], b"\0" * 8, "minisign key ID must not be empty")
        self.assertNotEqual(raw[10:], b"\0" * 32, "Ed25519 public key must not be empty")

    def test_production_key_is_not_a_test_fixture(self):
        encoded = update_public_key()
        for name in ("test.pub", "wrong.pub"):
            fixture = (ROOT / "tests/fixtures/update" / name).read_text(encoding="utf-8").strip()
            self.assertNotEqual(encoded, fixture, f"production key cannot be {name}")


if __name__ == "__main__":
    unittest.main()
