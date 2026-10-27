"""Testes dos PKGBUILDs do AUR; não exigem makepkg, pacman nem rede."""
import re
import shlex
import shutil
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AUR = ROOT / "packaging" / "aur"
PACKAGES = {"dish-pi-bin": AUR / "dish-pi-bin", "dish-pi": AUR / "dish-pi"}
REQUIRED_FIELDS = ("pkgname", "pkgver", "pkgrel", "arch", "url", "license", "source", "sha256sums")


def scalar_value(raw):
    raw = raw.strip()
    if raw.startswith(("'", '"')):
        parts = shlex.split(raw, comments=True)
        return parts[0] if parts else ""
    return raw.split("#", 1)[0].strip()


def read_array(text, start):
    """Devolve o corpo de um array, respeitando aspas e parênteses internos."""
    depth, quote, index = 1, None, start
    while index < len(text):
        character = text[index]
        if quote:
            if character == "\\":
                index += 2
                continue
            if character == quote:
                quote = None
        elif character in ("'", '"'):
            quote = character
        elif character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
            if depth == 0:
                return text[start:index]
        index += 1
    raise ValueError("array sem fechamento")


def parse_pkgbuild(text):
    """Lê atribuições simples e arrays no formato usado pelos PKGBUILDs."""
    values = {}
    assignments = re.compile(r"(?m)^([A-Za-z_][A-Za-z0-9_]*)\s*=")
    for match in assignments.finditer(text):
        key, cursor = match.group(1), match.end()
        while cursor < len(text) and text[cursor] in " \t":
            cursor += 1
        if cursor < len(text) and text[cursor] == "(":
            body = read_array(text, cursor + 1)
            values[key] = shlex.split(body, comments=True)
            continue
        end = text.find("\n", cursor)
        values[key] = scalar_value(text[cursor:end if end != -1 else len(text)])
    return values


def expand(value, values):
    for key in ("_release", "pkgver", "pkgname", "url"):
        if key in values:
            value = value.replace("${%s}" % key, values[key]).replace("$" + key, values[key])
    return value


def package_body(text):
    match = re.search(r"(?m)^package\(\)\s*\{", text)
    if not match:
        return ""
    body = text[match.end():]
    closing = body.rfind("\n}")
    return body[:closing] if closing != -1 else body


def parse_srcinfo(text):
    values = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition(" = ")
        if separator:
            values.setdefault(key, []).append(value)
    return values


class AurPackageTests(unittest.TestCase):
    def package_text(self, name):
        return (PACKAGES[name] / "PKGBUILD").read_text(encoding="utf-8")

    def package_values(self, name):
        return parse_pkgbuild(self.package_text(name))

    def srcinfo(self, name):
        path = PACKAGES[name] / ".SRCINFO"
        self.assertTrue(path.is_file(), f"{name}: .SRCINFO ausente")
        return parse_srcinfo(path.read_text(encoding="utf-8"))

    def test_required_fields_and_package_function(self):
        for name in PACKAGES:
            with self.subTest(package=name):
                values = self.package_values(name)
                for field in REQUIRED_FIELDS:
                    self.assertIn(field, values, f"{name}: campo {field} ausente")
                self.assertTrue(package_body(self.package_text(name)), f"{name}: package() ausente")

    def test_pkgname_does_not_collide_with_the_aur_dish(self):
        for name in PACKAGES:
            with self.subTest(package=name):
                values = self.package_values(name)
                self.assertEqual(values["pkgname"], name)
                self.assertNotEqual(values["pkgname"], "dish")
                self.assertNotIn("dish", values.get("provides", []))

    def test_checksums_are_real_and_not_skipped(self):
        for name in PACKAGES:
            with self.subTest(package=name):
                values = self.package_values(name)
                sources = values["source"]
                sums = values["sha256sums"]
                self.assertEqual(len(sources), len(sums), f"{name}: source e sha256sums não batem")
                self.assertNotIn("SKIP", [checksum.upper() for checksum in sums])
                for checksum in sums:
                    self.assertRegex(checksum, r"^[0-9a-f]{64}$")
                for source, checksum in zip(sources, sums):
                    self.assertTrue(expand(source, values), f"{name}: source vazio")
                    self.assertTrue(checksum, f"{name}: checksum vazio")

    def test_release_version_is_the_same_in_ci_and_pkgbuilds(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        match = re.search(r"--version ([0-9][^\s'\"]*)", workflow)
        self.assertIsNotNone(match, "ci.yml não declara a versão da release")
        release = match.group(1)
        for name in PACKAGES:
            with self.subTest(package=name):
                values = self.package_values(name)
                self.assertEqual(values["_release"], release, f"{name}: _release divergiu de ci.yml")
                # pkgver do Arch não pode conter hífen; a release usa hífen.
                self.assertEqual(values["pkgver"], release.replace("-", "."))
                self.assertNotIn("-", values["pkgver"])

    def test_binary_package_uses_the_release_archive(self):
        values = self.package_values("dish-pi-bin")
        release = values["_release"]
        source = expand(values["source"][0], values)
        self.assertIn(f"dish-{release}-linux-x86_64.tar.gz", source)
        self.assertIn(f"releases/download/v{release}/", source)
        self.assertIn("github.com/cleisonsantos/dish", source)

    def test_binary_package_depends_match_the_binary(self):
        values = self.package_values("dish-pi-bin")
        # Conjunto exato derivado do NEEDED do artefato da release:
        # libc.so.6/libm.so.6 -> glibc, libgcc_s.so.1 -> libgcc,
        # libxcb.so.1 -> libxcb, libxkbcommon*.so.0 -> libxkbcommon*.
        self.assertEqual(
            values["depends"],
            ["glibc", "libgcc", "libxcb", "libxkbcommon", "libxkbcommon-x11"],
        )
        # Estas bibliotecas não aparecem no ldd/NEEDED do artefato; não devem
        # ser dependências obrigatórias só porque estavam na proposta da issue.
        # Wayland, EGL e Vulkan são abertos com dlopen e ficam em optdepends.
        for dependency in ("openssl", "fontconfig", "freetype2", "libx11", "wayland"):
            self.assertNotIn(dependency, values["depends"])
        self.assertTrue(any(item.startswith("wayland:") for item in values["optdepends"]))
        self.assertTrue(any(item.startswith("libglvnd:") for item in values["optdepends"]))
        self.assertTrue(any(item.startswith("vulkan-icd-loader:") for item in values["optdepends"]))
        self.assertIn("dish-pi", values["provides"])
        self.assertIn("dish-pi", values["conflicts"])

    def test_binary_package_ships_the_release_binary_not_a_local_build(self):
        text = self.package_text("dish-pi-bin")
        values = self.package_values("dish-pi-bin")
        self.assertNotIn("cargo", text)
        self.assertNotIn("build()", text)
        self.assertEqual(values["arch"], ["x86_64"])
        # O binário do CI já vem remapeado e sem símbolos; não pode ser alterado.
        self.assertIn("!strip", values["options"])
        body = package_body(text)
        self.assertIn("$srcdir/dish-${_release}-linux-x86_64", body)
        self.assertNotIn("target/release", body)

    def test_source_package_declares_the_tag_build(self):
        text = self.package_text("dish-pi")
        values = self.package_values("dish-pi")
        release = values["_release"]
        self.assertIn("cargo build --release --locked", text)
        self.assertIn("cargo fetch --locked", text)
        self.assertIn(f"/archive/refs/tags/v{release}.tar.gz", expand(values["source"][0], values))
        for dependency in ("rust", "pkgconf", "fontconfig", "freetype2", "wayland"):
            self.assertIn(dependency, values["makedepends"])
        self.assertIn("dish-pi-bin", values["conflicts"])

    def test_source_and_binary_packages_declare_the_same_runtime_dependencies(self):
        self.assertEqual(
            self.package_values("dish-pi")["depends"],
            self.package_values("dish-pi-bin")["depends"],
        )

    def test_packages_install_the_expected_layout(self):
        for name in PACKAGES:
            with self.subTest(package=name):
                body = package_body(self.package_text(name))
                for path in (
                    "/usr/bin/dish-pi",
                    "/usr/share/applications/dish-pi.desktop",
                    "/usr/share/icons/hicolor/512x512/apps/dish-pi.png",
                    "/usr/share/licenses/dish-pi",
                    "/usr/share/doc/dish-pi/BUILD.json",
                ):
                    self.assertIn(path, body, f"{name}: não instala {path}")
                self.assertIn("Name=Dish", body)
                self.assertIn("Exec=dish-pi", body)
                self.assertIn("StartupWMClass=dish", body)

    @unittest.skipUnless(shutil.which("makepkg"), "makepkg não disponível")
    def test_srcinfo_is_what_makepkg_generates(self):
        for name, directory in PACKAGES.items():
            with self.subTest(package=name):
                result = subprocess.run(["makepkg", "--printsrcinfo"], cwd=directory,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((directory / ".SRCINFO").read_text(encoding="utf-8"),
                                 result.stdout, f"{name}: .SRCINFO desatualizado")

    def test_release_version_matches_cargo_toml_base_version(self):
        manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        base = re.search(r'(?m)^version = "([^"]+)"', manifest)
        self.assertIsNotNone(base, "Cargo.toml não declara version")
        for name in PACKAGES:
            with self.subTest(package=name):
                release = self.package_values(name)["_release"]
                self.assertTrue(release.startswith(base.group(1) + "-"),
                                f"{name}: _release {release} não parte de {base.group(1)}")

    def test_srcinfo_matches_pkgbuild(self):
        for name in PACKAGES:
            with self.subTest(package=name):
                values = self.package_values(name)
                srcinfo = self.srcinfo(name)
                self.assertEqual(srcinfo["pkgname"], [name])
                self.assertEqual(srcinfo["pkgver"], [values["pkgver"]])
                self.assertEqual(srcinfo["pkgrel"], [values["pkgrel"]])
                self.assertEqual(srcinfo["sha256sums"], values["sha256sums"])
                self.assertNotIn("SKIP", [checksum.upper() for checksum in srcinfo["sha256sums"]])

    @unittest.skipUnless(shutil.which("bash"), "bash não disponível")
    def test_pkgbuilds_are_valid_bash(self):
        for name, directory in PACKAGES.items():
            with self.subTest(package=name):
                result = subprocess.run(["bash", "-n", str(directory / "PKGBUILD")],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
