# Test-only release signatures

These keys are public test material. `test-private.pem` is the fixed Ed25519
private key used by `generate.py`; `test.pub` is the corresponding public key
in minisign base64 format. `wrong.pub` has the same key ID and a different
public key, to exercise cryptographic rejection rather than a decoding error.
Never use any of them for a release or embed them in the production client.

The `.json.minisig` files use the modern minisign `ED` format: detached
Ed25519 signature over BLAKE2b-512 of the exact JSON bytes, plus the signed
trusted comment. Rust tests verify these with minisign-verify 0.3.0 and
`allow_legacy = false`. The malformed JSON is intentionally signed correctly.

To regenerate, run `python3 tests/fixtures/update/generate.py` with Python
`cryptography` installed. Cargo tests consume the fixed files and need no
Python dependency, CLI signer, network access or real signing secrets.
