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
        self.assertEqual([p["id"] for p in data["providers"]], ["anthropic", "groq", "openai"])
        by_id = {p["id"]: p for p in data["providers"]}
        self.assertFalse(by_id["anthropic"]["configured"])
        self.assertFalse(by_id["groq"]["configured"])
        self.assertTrue(by_id["openai"]["configured"])
        self.assertEqual(by_id["openai"]["source"], "stored")
        self.assertEqual(by_id["openai"]["models"], 1)
        # `listCredentials` só expõe provedor e tipo.
        self.assertEqual(
            data["credentials"],
            [{"providerId": "groq", "type": "api_key"}, {"providerId": "openai", "type": "oauth"}],
        )
        self.assertTrue(data["readOnly"])

    def test_package_can_come_from_the_environment(self):
        result = run_adapter(["--status"], env={"DISH_PI_PACKAGE": str(FIXTURE)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads(result.stdout)["data"]["providers"]), 3)

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

    def test_jsonl_protocol_answers_status_and_refuses_writes(self):
        requests = "\n".join([
            json.dumps({"id": 1, "op": "status"}),
            json.dumps({"id": "a", "op": "login", "provider": "openai"}),
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
        # Escrever credencial é fase 3: recusa explícita, nunca meia implementação.
        self.assertEqual((replies[1]["id"], replies[1]["error"]), ("a", "not_implemented"))
        self.assertEqual((replies[2]["id"], replies[2]["error"]), (3, "not_implemented"))
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
