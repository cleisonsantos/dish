#!/usr/bin/env bash
# No installs, mutations of Cargo.lock, or publishing. See docs/security-audit.md.
set -uo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
exec python3 scripts/security_audit.py "$@"
