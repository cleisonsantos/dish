#!/usr/bin/env python3
"""Read-only security checks. Exit 1: findings; 2: incomplete; 3: both."""
import argparse
from collections import Counter
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

import license_bundle

ROOT = Path(__file__).resolve().parent.parent
PATTERNS = {
    "aws-access-key": rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
    "aws-secret-key": rb"(?i)\baws[_-]?secret[_-]?access[_-]?key[\"']?\s*[:=]\s*[\"']?[A-Za-z0-9/+=]{40}(?![A-Za-z0-9/+=])",
    "github-token": rb"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255})\b",
    "openai-anthropic-token": rb"\bsk-(?:(?:proj|svcacct|ant-api\d+)-)?[A-Za-z0-9_-]{20,255}\b",
    "private-key": rb"-----BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY-----",
    "url-password": rb"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s/:@]+:[^\s/@]+@",
}
RULES = {name: re.compile(pattern) for name, pattern in PATTERNS.items()}
# Explicitly reviewed SPDX options; unknown terms require review, not acceptance.
PERMISSIVE = set("MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib CC0-1.0 Unlicense Unicode-3.0 BSL-1.0 0BSD".split())


def run(*args):
    return subprocess.check_output(args, cwd=ROOT)


def cvss_severity(vector):
    """CVSS 3.x base severity; unsupported/missing vectors remain unknown."""
    if not vector or not vector.startswith(("CVSS:3.0/", "CVSS:3.1/")):
        return "unknown"
    try:
        v = dict(part.split(":") for part in vector.split("/")[1:])
        changed = v["S"] == "C"
        if v["S"] not in ("U", "C"):
            return "unknown"
        impact = 1 - math.prod(1 - {"H": .56, "L": .22, "N": 0}[v[k]] for k in "CIA")
        impact = 7.52 * (impact - .029) - 3.25 * (impact - .02) ** 15 if changed else 6.42 * impact
        exploit = (8.22 * {"N": .85, "A": .62, "L": .55, "P": .2}[v["AV"]]
                   * {"L": .77, "H": .44}[v["AC"]]
                   * {"N": .85, "L": .68 if changed else .62, "H": .5 if changed else .27}[v["PR"]]
                   * {"N": .85, "R": .62}[v["UI"]])
        score = 0 if impact <= 0 else math.ceil(round(min((impact + exploit) * (1.08 if changed else 1), 10), 5) * 10) / 10
        return "critical" if score >= 9 else "high" if score >= 7 else "medium" if score >= 4 else "low" if score > 0 else "none"
    except (ValueError, KeyError):
        return "unknown"


def license_allowed(expression, package):
    tokens = re.findall(r"[()]|[^\s()]+", expression.replace("/", " OR "))
    index = 0

    def atom():
        nonlocal index
        token = tokens[index]
        index += 1
        if token == "(":
            value = expr()
            if tokens[index] != ")":
                raise ValueError()
            index += 1
            return value
        value = token in PERMISSIVE or (token == "MPL-2.0" and package == "option-ext")
        if index < len(tokens) and tokens[index] == "WITH":
            index += 1
            value = token == "Apache-2.0" and tokens[index] == "LLVM-exception"
            index += 1
        return value

    def conjunction():
        nonlocal index
        value = atom()
        while index < len(tokens) and tokens[index] == "AND":
            index += 1
            other = atom()
            value = value and other
        return value

    def expr():
        nonlocal index
        value = conjunction()
        while index < len(tokens) and tokens[index] == "OR":
            index += 1
            other = conjunction()
            value = value or other
        return value

    try:
        value = expr()
        return value and index == len(tokens)
    except (ValueError, IndexError):
        return False


def dependencies():
    auditor = os.environ.get("CARGO_AUDIT") or shutil.which(
        "cargo-audit", path=os.pathsep.join([
            os.environ.get("PATH", ""), "/tmp/dish-security-tools/bin",
        ]))
    if not auditor:
        print("DEPENDENCIES INCOMPLETE: cargo-audit unavailable; critical/high/medium/low/unknown = N/A")
        return 2
    database = Path(os.environ.get("CARGO_AUDIT_DB", "~/.cargo/advisory-db")).expanduser()
    # --no-yanked avoids the separate crates.io index lookup; the advisory DB
    # is a local snapshot and must be provisioned outside this read-only check.
    result = subprocess.run([auditor, "audit", "--file", "Cargo.lock", "--json",
                             "--no-fetch", "--no-yanked", "--db", str(database)],
                            cwd=ROOT, capture_output=True)
    try:
        report = json.loads(result.stdout)
        entries = report["vulnerabilities"]["list"]
        counts = Counter()
        # Missing/unsupported CVSS is counted explicitly, never silently as low.
        for entry in entries:
            advisory = entry["advisory"]
            severity = advisory.get("severity") or cvss_severity(advisory.get("cvss"))
            counts[severity.lower()] += 1
            print("ADVISORY", entry["package"]["name"], entry["package"]["version"],
                  advisory["id"], "severity=" + severity, "cvss=" + str(advisory.get("cvss")),
                  "reachability=requires-review decision=requires-review")
        warnings = report.get("warnings", {})
        for kind, items in warnings.items():
            for entry in items:
                print("WARNING", kind, entry.get("package"), entry.get("advisory", {}).get("id"))
        print("DEPENDENCIES", " ".join(f"{s}={counts[s]}" for s in ("critical", "high", "medium", "low", "none", "unknown")))
        print("CARGO_AUDIT_JSON", json.dumps(report, sort_keys=True))
        if entries or any(warnings.values()):
            return 1 if result.returncode in (0, 1) else 3
        return 0 if result.returncode == 0 else 2
    except (ValueError, KeyError, TypeError):
        print("DEPENDENCIES INCOMPLETE: cargo audit failed; severity counts = N/A; exit", result.returncode)
        return 2


def licenses():
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    expected = {(p["name"], p["version"], p.get("source")) for p in lock if p.get("source")}
    args = ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"]
    result = subprocess.run(args, cwd=ROOT, capture_output=True)
    status = 0
    if result.returncode:
        print("LICENSES: full offline metadata unavailable; trying Linux inventory")
        result = subprocess.run(args + ["--filter-platform", "x86_64-unknown-linux-gnu"], cwd=ROOT, capture_output=True)
    if result.returncode:
        print("LICENSES INCOMPLETE: cargo metadata unavailable")
        return 2
    metadata = json.loads(result.stdout)
    packages = [p for p in metadata["packages"] if p["id"] != metadata["resolve"]["root"]]
    present = {(p["name"], p["version"], p.get("source")) for p in packages}
    missing = sorted(expected - present)
    for name, version, source in missing:
        print("LICENSE_UNVERIFIED", name, version, source)
    if missing:
        status |= 2
    collected = 0
    with tempfile.TemporaryDirectory(prefix="dish-license-audit-") as tmp:
        for p in sorted(packages, key=lambda p: (p["name"], p["version"])):
            if not license_allowed(p.get("license") or "", p["name"]):
                print("LICENSE_REVIEW", p["name"], p["version"], p.get("license"))
                status |= 1
            try:
                # Exercise the actual release collector, including native hashes and source archives.
                license_bundle.collect_licenses({"resolve": metadata["resolve"], "packages": [p]}, Path(tmp), ROOT)
                collected += 1
            except (RuntimeError, OSError) as error:
                print("LICENSE_GAP", p["name"], p["version"], str(error))
                status |= 1
    print(f"LICENSES lock_dependencies={len(expected)} inventory={len(packages)} collected={collected} unverified={len(missing)} status={status}")
    return status


def matches(data):
    if b"\0" in data:
        return []
    return [(name, data[:match.start()].count(b"\n") + 1)
            for name, rule in RULES.items() for match in rule.finditer(data)]


def secrets():
    findings = 0
    files = 0
    binaries = 0
    # Only locations and rule names leave this process. Never print matching text.
    for raw in run("git", "ls-files", "-z").split(b"\0"):
        if not raw:
            continue
        path = Path(os.fsdecode(raw))
        if "target" in path.parts:
            continue
        full = ROOT / path
        if full.is_symlink():
            data = os.fsencode(os.readlink(full))
        elif full.is_file():
            data = full.read_bytes()
        else:
            continue
        files += 1
        binaries += b"\0" in data
        for rule, line in matches(data):
            print("SECRET_CANDIDATE", "worktree", json.dumps(str(path)), f"line={line}", rule)
            findings += 1
    command = ["git", "log", "--all", "--full-history", "--root", "-m", "-p", "--no-ext-diff",
               "--no-textconv", "--no-renames", "--format=COMMIT %H", "--", ".", ":(exclude)target/**", ":(exclude)**/target/**"]
    commit, path, line_number = "", "", 0
    with subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE) as process:
        for line in process.stdout:
            if line.startswith(b"COMMIT "):
                commit = line.strip().split()[-1].decode("ascii")
            elif line.startswith(b"diff --git "):
                path = line.decode("utf-8", "replace").strip()
                line_number = 0
            elif line.startswith((b"+", b"-", b" ")) and not line.startswith((b"+++", b"---")):
                line_number += 1
                for rule, _ in matches(line[1:]):
                    print("SECRET_CANDIDATE", commit, json.dumps(path), f"diff_line={line_number}", rule)
                    findings += 1
        if process.wait():
            return 2 | bool(findings)
    commits = run("git", "rev-list", "--all", "--count").decode().strip()
    shallow = run("git", "rev-parse", "--is-shallow-repository").strip() == b"true"
    print(f"SECRETS tracked_files={files} binary_files_skipped={binaries} commits={commits} candidates={findings} shallow={shallow}")
    return int(bool(findings)) | (2 if shallow else 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=["dependencies", "licenses", "secrets"])
    args = parser.parse_args()
    print("COMMIT", run("git", "rev-parse", "HEAD").decode().strip())
    print("LOCK_SHA256", license_bundle.sha256(ROOT / "Cargo.lock"))
    status = 0
    for name, check in [("dependencies", dependencies), ("licenses", licenses), ("secrets", secrets)]:
        if args.only and args.only != name:
            continue
        try:
            status |= check()
        except (OSError, subprocess.SubprocessError, ValueError) as error:
            print(name.upper(), "INCOMPLETE", type(error).__name__)
            status |= 2
    print("AUDIT_EXIT", status, "(0=clean, 1=findings, 2=incomplete, 3=both)")
    return status


if __name__ == "__main__":
    raise SystemExit(main())
