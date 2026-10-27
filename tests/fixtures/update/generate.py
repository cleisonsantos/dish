#!/usr/bin/env python3
"""Regenerate test-only minisign fixtures. Requires Python cryptography, not used by cargo tests."""
import base64
import hashlib
import json
from pathlib import Path
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).parent
# Public test material. This key must never be trusted by the application.
key = Ed25519PrivateKey.from_private_bytes(bytes(range(32)))
(ROOT / 'test-private.pem').write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
key_id = bytes.fromhex('1180000000000000')
(ROOT / 'test.pub').write_text(base64.b64encode(b'Ed' + key_id + public).decode() + '\n')
wrong_public = Ed25519PrivateKey.from_private_bytes(bytes(reversed(range(32)))).public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
(ROOT / 'wrong.pub').write_text(base64.b64encode(b'Ed' + key_id + wrong_public).decode() + '\n')

def sign(name, data):
    (ROOT / name).write_bytes(data)
    signature = key.sign(hashlib.blake2b(data, digest_size=64).digest())
    comment = b'test fixture only'
    encoded = base64.b64encode(b'ED' + key_id + signature).decode()
    global_sig = base64.b64encode(key.sign(signature + comment)).decode()
    (ROOT / (name + '.minisig')).write_text(f'untrusted comment: test fixture only\n{encoded}\ntrusted comment: {comment.decode()}\n{global_sig}\n')

for version in ['0.1.0-alpha.5', '0.1.0-alpha.6', '0.1.0-alpha.7', '0.1.0-alpha.4', '0.1.0']:
    tag = 'v' + version
    name = f'dish-{version}-linux-x86_64.tar.gz'
    manifest = dict(schema=1, version=version, tag=tag, channel='prerelease' if '-' in version else 'stable',
                    published_at='2026-10-26T12:00:00Z', notes_url=f'https://github.com/cleisonsantos/dish/releases/tag/{tag}',
                    target='x86_64-unknown-linux-gnu', minimum_glibc='2.39',
                    assets=[dict(kind='portable-linux-x86_64', name=name, size=123,
                                 sha256='a' * 64, url=f'https://github.com/cleisonsantos/dish/releases/download/{tag}/{name}')])
    sign(version + '.json', (json.dumps(manifest, indent=2) + '\n').encode())
sign('invalid.json', b'{this is not JSON}\n')
sign('invalid-version.json', json.dumps(dict(manifest, version='0.1')).encode())
sign('missing-assets.json', json.dumps(dict(manifest, assets=[])).encode())
