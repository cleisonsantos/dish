"""Contrato do adaptador de autenticação: estado e origem, nunca um segredo.

Usa o SDK falso em tests/fixtures/fake-pi-sdk, que esconde segredos em campos
que o adaptador não pode copiar. Não abre a credencial real, não usa rede e não
depende do Pi instalado — só de um Node no PATH (o mesmo que o Pi exige).
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "scripts/pi-auth-adapter.mjs"
FIXTURE = ROOT / "tests/fixtures/fake-pi-sdk"
# Valores plantados na fixture: nenhum pode aparecer na saída do adaptador.
PLANTED = ("FIXTURE", "sk-ant-", "eyJhbGciOi", "sk-live-")

requires_node = unittest.skipUnless(shutil.which("node"), "node is required for the adapter")


def run_adapter(args, stdin=None, env=None):
    merged = dict(os.environ)
    merged.pop("DISH_PI_PACKAGE", None)
    merged.pop("DISH_PI_BIN", None)
    if env:
        merged.update(env)
    return subprocess.run(
        ["node", str(ADAPTER), *args],
        input=stdin,
        capture_output=True,
        text=True,
        env=merged,
        timeout=60,
    )


@requires_node
class ProviderAdapterTests(unittest.TestCase):
    def test_status_lists_the_catalog_without_credentials_or_secrets(self):
        result = run_adapter(["--status", "--package", str(FIXTURE)])
        self.assertEqual(result.returncode, 0, result.stderr)
        for planted in PLANTED:
            self.assertNotIn(planted, result.stdout, "segredo vazou na saída")
        data = json.loads(result.stdout)["data"]
        # O catálogo inclui provedor sem credencial: é o motivo da issue #69.
        self.assertEqual(
            [p["id"] for p in data["providers"]],
            ["anthropic", "groq", "openai", "select-provider"],
        )
        by_id = {p["id"]: p for p in data["providers"]}
        self.assertFalse(by_id["anthropic"]["configured"])
        self.assertFalse(by_id["groq"]["configured"])
        self.assertTrue(by_id["openai"]["configured"])
        self.assertEqual(by_id["openai"]["source"], "stored")
        self.assertEqual(by_id["openai"]["models"], 1)
        # `listCredentials` só expõe provedor e tipo.
        self.assertEqual(data["credentials"], [{"providerId": "openai", "type": "oauth"}])
        self.assertFalse(data["readOnly"])
        # Quem pode ser conectado, e com qual nome o provedor se apresenta.
        by_id = {p["id"]: p for p in data["providers"]}
        self.assertEqual(by_id["anthropic"]["auth"]["api_key"]["name"], "Anthropic API key")
        self.assertIsNone(by_id["anthropic"]["auth"]["oauth"])
        self.assertEqual(by_id["openai"]["auth"]["oauth"]["label"], "Entrar com a conta ChatGPT")
        self.assertTrue(by_id["openai"]["auth"]["oauth"]["subscription"])
        # Provedor sem login interativo não oferece botão nenhum.
        self.assertIsNone(by_id["groq"]["auth"]["api_key"])

    def test_package_can_come_from_the_environment(self):
        result = run_adapter(["--status"], env={"DISH_PI_PACKAGE": str(FIXTURE)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads(result.stdout)["data"]["providers"]), 4)

    def test_a_stated_package_is_authoritative(self):
        result = run_adapter(["--status", "--package", "/nonexistent/pi-package"])
        self.assertNotEqual(result.returncode, 0)
        body = json.loads(result.stdout)
        self.assertFalse(body["ok"])
        # Caminho explícito errado não cai para outra instalação em silêncio.
        self.assertEqual(body["error"], "package_invalid")

    def test_an_environment_package_is_also_authoritative(self):
        result = run_adapter(["--status"], env={"DISH_PI_PACKAGE": "/nonexistent/pi-package"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)["error"], "package_invalid")

    @unittest.skipUnless(hasattr(os, "symlink") and os.name != "nt", "symlinks are required")
    def test_pi_bin_walks_from_the_symlinked_shim_to_the_scoped_package(self):
        # npm deixa `bin/pi` como link para dentro do pacote; o `basename` do
        # alvo é `pi-coding-agent`, não o nome com escopo.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            package = root / "lib/node_modules/@earendil-works/pi-coding-agent"
            shutil.copytree(FIXTURE, package)
            shim = root / "bin/pi"
            shim.parent.mkdir(parents=True)
            shim.symlink_to(package / "dist/index.js")
            result = run_adapter(
                ["--status", "--pi-bin", str(shim)],
                env={
                    "HOME": str(root / "home"),
                    "PI_CODING_AGENT_DIR": str(root / "agent"),
                    "XDG_DATA_HOME": str(root / "data"),
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(json.loads(result.stdout)["data"]["providers"]), 4)

    def test_windows_data_and_npm_candidates_are_searched(self):
        # Simula o ambiente do Windows: sem XDG_DATA_HOME, o prefixo do Dish
        # fica em LOCALAPPDATA e o npm global em APPDATA. Nenhum dos dois era
        # procurado antes da correção.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            local = root / "AppData/Local"
            roaming = root / "AppData/Roaming"
            dish_package = local / "dish/pi/lib/node_modules/@earendil-works/pi-coding-agent"
            npm_package = roaming / "npm/node_modules/@earendil-works/pi-coding-agent"
            for package in (dish_package, npm_package):
                (package / "dist").mkdir(parents=True)
                (package / "dist/index.js").write_text("export {};\n")
            script = (
                f"import {{ packageCandidates }} from {json.dumps(ADAPTER.as_uri())};\n"
                f"const candidates = packageCandidates({{ env: {json.dumps({'LOCALAPPDATA': str(local), 'APPDATA': str(roaming)})}, home: {json.dumps(str(root))} }});\n"
                "console.log(JSON.stringify(candidates));\n"
            )
            result = subprocess.run(
                ["node", "--input-type=module", "-e", script],
                capture_output=True, text=True, timeout=60,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            body = json.loads(result.stdout)
            self.assertIsNone(body["rejected"])
            dirs = {Path(directory) for directory in body["dirs"]}
            self.assertIn(dish_package, dirs)
            self.assertIn(npm_package, dirs)

    @unittest.skipUnless(hasattr(os, "symlink") and os.name != "nt", "symlinks are required")
    def test_an_npm_global_prefix_is_found_without_a_symlink(self):
        # Prefixo npm global com um shim de verdade (no Windows é um .cmd, não
        # um link): o caminho `<prefixo>/bin/pi` ainda aponta para o pacote.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            package = root / "lib/node_modules/@earendil-works/pi-coding-agent"
            shutil.copytree(FIXTURE, package)
            shim = root / "bin/pi"
            shim.parent.mkdir(parents=True)
            shim.write_text("#!/bin/sh\nexit 0\n")
            shim.chmod(0o700)
            result = run_adapter(
                ["--status", "--pi-bin", str(shim)],
                env={
                    "HOME": str(root / "home"),
                    "PI_CODING_AGENT_DIR": str(root / "agent"),
                    "XDG_DATA_HOME": str(root / "data"),
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(json.loads(result.stdout)["data"]["providers"]), 4)

    @unittest.skipUnless(os.name != "nt" and shutil.which("npm"), "npm is required")
    def test_npm_root_g_is_the_last_resort(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            package = root / "global/lib/node_modules/@earendil-works/pi-coding-agent"
            shutil.copytree(FIXTURE, package)
            bin_dir = root / "fake-bin"
            bin_dir.mkdir()
            npm = bin_dir / "npm"
            npm.write_text(f"#!/bin/sh\nprintf '%s\\n' '{root / 'global/lib/node_modules'}'\n")
            npm.chmod(0o700)
            result = run_adapter(
                ["--status"],
                env={
                    "HOME": str(root / "home"),
                    "PATH": f"{bin_dir}{os.pathsep}{os.environ.get('PATH', '')}",
                    "PI_CODING_AGENT_DIR": str(root / "agent"),
                    "XDG_DATA_HOME": str(root / "data"),
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(json.loads(result.stdout)["data"]["providers"]), 4)

    def test_jsonl_protocol_answers_status_and_rejects_unknown_ops(self):
        # O ciclo interativo de login tem teste próprio (test_provider_login.py);
        # aqui fica só o que não depende de interação.
        requests = "\n".join([
            json.dumps({"id": 1, "op": "status"}),
            json.dumps({"id": "a", "op": "login"}),
            json.dumps({"id": 3, "op": "logout"}),
            json.dumps({"id": 4, "op": "wat"}),
            "not json",
        ]) + "\n"
        result = run_adapter(["--package", str(FIXTURE)], stdin=requests)
        self.assertEqual(result.returncode, 0, result.stderr)
        for planted in PLANTED:
            self.assertNotIn(planted, result.stdout)
        replies = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        self.assertEqual(len(replies), 5)
        self.assertEqual(replies[0]["id"], 1)
        self.assertTrue(replies[0]["ok"])
        # Sem provedor não há o que fazer: recusa fechada, não silêncio.
        self.assertEqual((replies[1]["id"], replies[1]["error"]), ("a", "bad_request"))
        self.assertEqual((replies[2]["id"], replies[2]["error"]), (3, "bad_request"))
        self.assertEqual((replies[3]["id"], replies[3]["error"]), (4, "unsupported_op"))
        self.assertIsNone(replies[4]["id"])
        self.assertEqual(replies[4]["error"], "bad_json")

    def test_the_adapter_never_prints_a_prompt_answer(self):
        # Mesmo caminho de erro: a mensagem do SDK não pode carregar o segredo.
        result = run_adapter(["--package", str(FIXTURE)], stdin=json.dumps({"id": 1, "op": "status"}) + "\n")
        self.assertEqual(result.returncode, 0)
        self.assertNotIn("access", result.stdout)


if __name__ == "__main__":
    unittest.main()
