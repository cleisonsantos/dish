#!/usr/bin/env python3
"""Build an AppImage from the Linux candidate archive; never upload it."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('package_deb', ROOT / 'scripts/package-deb.py')
deb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deb)


def artifact_name(label):
    return f'dish-{label}-x86_64.AppImage'


def desktop_entry():
    return deb.desktop_entry().replace('Exec=dish-pi\n', 'Exec=dish\n').replace('Icon=dish-pi\n', 'Icon=dish\n')


def stage_tree(package, appdir):
    (appdir / 'usr/bin').mkdir(parents=True)
    shutil.copyfile(package / 'bin/dish', appdir / 'usr/bin/dish')
    (appdir / 'usr/bin/dish').chmod(0o755)
    shutil.copyfile(package / 'share/icons/dish.png', appdir / 'dish.png')
    (appdir / 'dish.desktop').write_text(desktop_entry())
    (appdir / 'AppRun').write_text('#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/dish" "$@"\n')
    (appdir / 'AppRun').chmod(0o755)
    shutil.copytree(package / 'share/licenses', appdir / 'usr/share/doc/dish/licenses')


def build(archive, output_dir, label):
    tool = shutil.which('appimagetool')
    if tool is None:
        raise RuntimeError('appimagetool is required to build an AppImage')
    checksum = Path(str(archive) + '.sha256')
    if checksum.is_file() and deb.sha256(archive) != checksum.read_text().split()[0]:
        raise RuntimeError(f'Checksum mismatch for {archive}')
    output_dir.mkdir(parents=True, exist_ok=True)
    output = (output_dir / artifact_name(label)).resolve()
    with tempfile.TemporaryDirectory(prefix='dish-appimage-') as tmp:
        root = Path(tmp)
        with tarfile.open(archive) as tar:
            tar.extractall(root / 'unpacked', filter='data')
        entries = list((root / 'unpacked').iterdir())
        if len(entries) != 1 or not entries[0].is_dir():
            raise RuntimeError('The candidate archive must contain a single top-level directory')
        package = entries[0]
        info = json.loads((package / 'BUILD.json').read_text())
        if info.get('target') != deb.TARGET or info.get('version') != label:
            raise RuntimeError('Candidate target or version does not match the requested label')
        appdir = root / 'Dish.AppDir'
        stage_tree(package, appdir)
        # The CI runner has no FUSE; extract the tool itself before running it.
        subprocess.run([tool, '--appimage-extract-and-run', str(appdir), str(output)], check=True,
                       env={**os.environ, 'ARCH': 'x86_64'})
    output.chmod(output.stat().st_mode | 0o111)
    Path(str(output) + '.sha256').write_text(f'{deb.sha256(output)}  {output.name}\n')
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version')
    parser.add_argument('--input')
    parser.add_argument('--output', default=str(ROOT / 'dist'))
    args = parser.parse_args()
    base = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    label = args.version or base
    if not re.fullmatch(re.escape(base) + r'(?:-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)?', label):
        parser.error('Version must match Cargo.toml, optionally with a prerelease suffix')
    archive = Path(args.input) if args.input else ROOT / 'dist' / f'dish-{label}-linux-x86_64.tar.gz'
    if not archive.is_file():
        parser.error(f'Candidate archive not found: {archive}')
    print(f'AppImage: {build(archive, Path(args.output), label)}')


if __name__ == '__main__':
    main()
