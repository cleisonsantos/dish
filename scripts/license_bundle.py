#!/usr/bin/env python3
"""Collect the dependency license bundle shared by every Dish release.

The bundle is platform-independent: it walks the packages reported by
`cargo metadata` for the target being packaged, copies their license files
(plus the hash-pinned overrides and native notices), and adds the Rust
toolchain notices. Linux, Windows and macOS packaging all use this module so
the three releases carry the same obligations.
"""
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import subprocess
import time
import tomllib
import urllib.error
import urllib.request

LICENSE_NAME = re.compile(r"^(licen[sc]e|copying|copyright|notice)([._-].*|$)", re.I)

# Textos padrão para crates publicadas sem nenhum arquivo de licença. Só são
# usados quando o pacote não traz nada e não existe override; a crate completa
# (com seus avisos de copyright) entra junto no bundle.
FALLBACK_TEXTS = {
    "MIT": "licenses/standard/MIT.txt",
    "Apache-2.0": "LICENSE",
    "CC0-1.0": "licenses/standard/CC0-1.0.txt",
}
SPDX_PARTS = re.compile(r"\s+(?:OR|AND)\s+|[()]")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fallback_texts(license_expr: str, root: Path) -> list[str]:
    """Textos conhecidos que cobrem a expressão SPDX declarada, se houver.

    Devolve vazio quando algum identificador é desconhecido: nesse caso o build
    falha de propósito, porque não dá para garantir os termos.
    """
    chosen = []
    for part in SPDX_PARTS.split(license_expr):
        part = (part or "").strip()
        if not part:
            continue
        relative = FALLBACK_TEXTS.get(part)
        if relative is None or not (root / relative).is_file():
            return []
        if relative not in chosen:
            chosen.append(relative)
    return chosen


def crate_archive(package, dest: Path, lock, root: Path) -> str:
    source_dir = Path(package["manifest_path"]).parent
    registry = source_dir.parent.name
    archive = source_dir.parents[2] / "cache" / registry / f'{package["name"]}-{package["version"]}.crate'
    entry = next((p for p in lock["package"] if p["name"] == package["name"]
                  and p["version"] == package["version"] and p.get("source") == package["source"]), None)
    if not entry or not archive.is_file() or sha256(archive) != entry.get("checksum"):
        raise RuntimeError(f'Cannot verify original source archive: {package["name"]} {package["version"]}')
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(archive, dest / archive.name)
    return archive.name


def collect_licenses(metadata, destination: Path, root: Path) -> int:
    overrides = {(p["name"], p["version"]): p for p in json.loads((root / "licenses/overrides.json").read_text(encoding="utf-8"))}
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    native_path = root / "licenses/native.json"
    native = {(p["name"], p["version"]): p for p in json.loads(native_path.read_text(encoding="utf-8"))} if native_path.exists() else {}
    packages = []
    for p in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if p["id"] == metadata["resolve"]["root"]:
            continue
        if not p.get("license"):
            raise RuntimeError(f'No declared license: {p["name"]}')
        source = Path(p["manifest_path"]).parent
        files = sorted(f for f in source.rglob("*") if f.is_file() and LICENSE_NAME.match(f.name))
        folder = destination / "dependencies" / f'{p["name"]}-{p["version"]}'
        override = None
        copied = []
        if files:
            for f in files:
                relative = f.relative_to(source)
                target = folder / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(str(relative))
        else:
            override = overrides.get((p["name"], p["version"]))
            if override is None:
                # Versões diferentes entre plataformas aparecem aqui (o Windows
                # resolve alguns crates em outra versão). Antes de falhar, usa os
                # textos padrão das licenças declaradas e leva junto a crate
                # completa, que preserva os avisos de copyright.
                fallback = fallback_texts(p.get("license", ""), root)
                if not fallback:
                    raise RuntimeError(f'Missing license texts: {p["name"]} {p["version"]}; review licenses/overrides.json')
                for i, relative in enumerate(fallback):
                    f = (root / relative).resolve()
                    if not f.is_file() or not f.is_relative_to(root):
                        raise RuntimeError(f'Invalid standard license text for {p["name"]}: {relative}')
                    target = folder / f'{i}-{f.name}'
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(f, target)
                    copied.append(target.name)
                override = {"standard_texts": fallback,
                            "reason": "published crate without license files"}
            for i, entry in enumerate(override.get("files", [])):
                f = (root / entry["path"]).resolve()
                if not f.is_relative_to(root) or sha256(f) != entry["sha256"]:
                    raise RuntimeError(f'Invalid license override for {p["name"]}')
                target = folder / f'{i}-{f.name}'
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(target.name)
        native_entry = native.get((p["name"], p["version"]))
        if p["name"] == "freetype-sys" and not native_entry:
            raise RuntimeError("Review the new FreeType native license inputs in licenses/native.json")
        if native_entry:
            for entry in native_entry["files"]:
                f = (source / entry["path"]).resolve()
                if not f.is_relative_to(source) or sha256(f) != entry["sha256"]:
                    raise RuntimeError(f'Invalid native license input for {p["name"]}')
                target = folder / entry["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(f, target)
                copied.append(entry["path"])
        record = {k: p.get(k) for k in ["name", "version", "license", "repository", "authors"]}
        record["files"] = copied
        if native_entry:
            record["native"] = native_entry
        if p["name"] == "self_cell":
            record["selected_license"] = "Apache-2.0"
        if override:
            record["override"] = override
        needs_source = bool(override and override.get("standard_texts"))
        if p["name"] == "option-ext" or (override and override.get("include_source")) or needs_source or (native_entry and native_entry.get("include_source")):
            record["source_archive"] = crate_archive(p, destination / "sources", lock, root)
        packages.append(record)
    (destination / "dependencies.json").write_text(json.dumps(packages, indent=2) + "\n", encoding="utf-8")
    fallbacks = [f'{p["name"]} {p["version"]}' for p in packages if p.get("override", {}).get("standard_texts")]
    if fallbacks:
        print("warning: standard license texts used for: " + ", ".join(sorted(fallbacks)))
    return len(packages)


# Status transitórios que valem uma nova tentativa. 403/404 e afins são erros
# reais (arquivo ou commit que não existe) e falham na primeira tentativa.
RETRYABLE_STATUS = frozenset({429, 500, 502, 503, 504})
DOWNLOAD_ATTEMPTS = 5
RETRY_AFTER_CAP = 60.0


def license_cache_dir() -> Path:
    """Cache local dos downloads, fora do repositório versionado."""
    return Path(os.environ.get("DISH_LICENSE_CACHE", Path.home() / ".cache/dish/licenses"))


def _cache_path(cache_dir: Path, url: str) -> Path:
    return cache_dir / f"{hashlib.sha256(url.encode('utf-8')).hexdigest()}.bin"


def _valid_cache(path: Path) -> bool:
    """Cache vazio é download interrompido, não uma cópia válida."""
    return path.is_file() and path.stat().st_size > 0


def _retry_after(error: urllib.error.HTTPError) -> float | None:
    value = error.headers.get("Retry-After") if error.headers else None
    if value is None:
        return None
    try:
        return max(0.0, float(int(value.strip())))
    except (AttributeError, ValueError):
        return None


def _retry_delay(attempt: int, retry_after: float | None, base_delay: float, cap: float) -> float:
    if retry_after is not None:
        return min(retry_after, cap)
    delay = min(base_delay * (2 ** attempt), cap)
    if not delay:
        return 0.0
    # Jitter evita que runners que falharam juntos voltem juntos (thundering herd).
    return min(delay + random.uniform(0, delay / 2), cap)


def download(url: str, *, attempts: int = DOWNLOAD_ATTEMPTS, timeout: float = 60,
             cache_dir: Path | None = None, base_delay: float = 1.0,
             retry_after_cap: float = RETRY_AFTER_CAP, sleep=None) -> bytes:
    """Baixa `url` com retry exponencial e cache local por URL.

    Reusar o cache não enfraquece a proveniência: `toolchain_licenses` grava no
    `provenance.json` o sha256 do conteúdo que efetivamente entrou no bundle, e
    a URL já carrega o commit pinado do compilador. Sem cache e sem download, o
    build falha de propósito - bundle de licenças incompleto não deve ser
    publicado.
    """
    if attempts < 1:
        raise ValueError("attempts must be >= 1")
    if sleep is None:
        sleep = time.sleep
    cache = _cache_path(cache_dir or license_cache_dir(), url)
    if _valid_cache(cache):
        return cache.read_bytes()
    last_error = "unknown error"
    for attempt in range(attempts):
        retry_after = None
        try:
            with urllib.request.urlopen(url, timeout=timeout) as response:
                data = response.read()
        except urllib.error.HTTPError as error:
            # HTTPError carrega um fp (a resposta); fechar devolve o socket mesmo
            # quando o erro é propagado para o chamador.
            try:
                if error.code not in RETRYABLE_STATUS:
                    raise
                last_error = f"HTTP {error.code}"
                retry_after = _retry_after(error)
            finally:
                error.close()
        except (urllib.error.URLError, TimeoutError) as error:
            last_error = str(getattr(error, "reason", error))
        else:
            cache.parent.mkdir(parents=True, exist_ok=True)
            temporary = cache.with_name(f"{cache.name}.{os.getpid()}.tmp")
            try:
                temporary.write_bytes(data)
                temporary.replace(cache)
            finally:
                temporary.unlink(missing_ok=True)
            return data
        if attempt + 1 < attempts:
            delay = _retry_delay(attempt, retry_after, base_delay, retry_after_cap)
            if delay > 0:
                sleep(delay)
    if _valid_cache(cache):
        return cache.read_bytes()
    raise RuntimeError(
        f"Network failure downloading {url} after {attempts} attempts ({last_error}); "
        "no cached copy available - this is not a missing license")


def toolchain_licenses(destination: Path) -> None:
    """Rust toolchain and standard-library notices for the installed compiler."""
    def run(*args):
        return subprocess.check_output(args, text=True, encoding="utf-8")

    info = dict(line.split(": ", 1) for line in run("rustc", "-vV").splitlines() if ": " in line)
    commit = info.get("commit-hash", "")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise RuntimeError("Cannot identify Rust toolchain source commit")
    folder = destination / "rust-toolchain"
    folder.mkdir(parents=True, exist_ok=True)
    sources = []
    # Exact toolchain commit, never a moving branch. No remote code is executed.
    for name in ["LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT"]:
        url = f"https://raw.githubusercontent.com/rust-lang/rust/{commit}/{name}"
        data = download(url)
        (folder / name).write_bytes(data)
        sources.append({"file": name, "url": url, "sha256": hashlib.sha256(data).hexdigest()})
    documentation = Path(run("rustc", "--print", "sysroot").strip()) / "share/doc/rust"
    copyright_library = documentation / "COPYRIGHT-library.html"
    bundled_licenses = documentation / "licenses"
    if not copyright_library.is_file() or not bundled_licenses.is_dir():
        raise RuntimeError("Rust standard-library notices are missing; install the rust-docs component")
    shutil.copyfile(copyright_library, folder / "COPYRIGHT-library.html")
    shutil.copytree(bundled_licenses, folder / "licenses", dirs_exist_ok=True)
    sources.append({"file": "COPYRIGHT-library.html", "source": "installed Rust toolchain standard-library notices",
                    "sha256": sha256(folder / "COPYRIGHT-library.html")})
    (folder / "provenance.json").write_text(json.dumps({"rustc": info["release"], "files": sources}, indent=2) + "\n", encoding="utf-8")


def build(metadata, destination: Path, root: Path) -> int:
    """Collect everything a release archive must ship under `destination`."""
    destination.mkdir(parents=True, exist_ok=True)
    count = collect_licenses(metadata, destination, root)
    toolchain_licenses(destination)
    return count
