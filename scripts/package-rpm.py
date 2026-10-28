#!/usr/bin/env python3
"""Build an RPM from the Linux candidate archive; never upload it."""
import argparse
import importlib.util
import json
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
    return f'dish-{label}-1.x86_64.rpm'


def rpm_version(label):
    return deb.debian_version(label)


def spec_text(version, glibc):
    return f'''Name: dish-pi
Version: {version}
Release: 1
Summary: Desktop interface for the Pi coding agent
License: Apache-2.0
URL: {deb.HOMEPAGE}
BuildArch: x86_64
Requires: glibc >= {glibc}

%description
Dish is a desktop client for the Pi coding agent.

%install
mkdir -p "%{{buildroot}}"
cp -a "%{{dish_staging}}/." "%{{buildroot}}/"

%files
/usr/bin/dish-pi
/usr/share/applications/dish-pi.desktop
/usr/share/icons/hicolor/512x512/apps/dish-pi.png
/usr/share/doc/dish-pi
'''


def build(archive, output_dir, label):
    if shutil.which('rpmbuild') is None:
        raise RuntimeError('rpmbuild is required to build an RPM package')
    checksum = Path(str(archive) + '.sha256')
    if checksum.is_file() and deb.sha256(archive) != checksum.read_text().split()[0]:
        raise RuntimeError(f'Checksum mismatch for {archive}')
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='dish-rpm-') as tmp:
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
        glibc = str(info['minimum_glibc'])
        if not re.fullmatch(r'\d+(?:\.\d+)+', glibc):
            raise RuntimeError('Invalid minimum glibc version')
        staging = root / 'buildroot'
        deb.stage_tree(package, staging, rpm_version(label), glibc, [])
        shutil.rmtree(staging / 'DEBIAN')
        top = root / 'rpmbuild'
        top.mkdir()
        specfile = root / 'dish.spec'
        specfile.write_text(spec_text(rpm_version(label), glibc))
        subprocess.run(['rpmbuild', '-bb', '--define', f'_topdir {top}', '--define', f'dish_staging {staging}', str(specfile)], check=True)
        built = list((top / 'RPMS/x86_64').glob('*.rpm'))
        if len(built) != 1:
            raise RuntimeError('Expected exactly one RPM')
        output = output_dir / artifact_name(label)
        shutil.copyfile(built[0], output)
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
    print(f'RPM package: {build(archive, Path(args.output), label)}')


if __name__ == '__main__':
    main()
