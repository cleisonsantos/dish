"""Protocolo interativo do adaptador: login por chave, OAuth, cancelar e remover.

Usa o SDK falso em tests/fixtures/fake-pi-sdk, que guarda credenciais em memória.
Não abre rede, não toca no `auth.json` real e não depende do Pi instalado.
"""
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "scripts/pi-auth-adapter.mjs"
FIXTURE = ROOT / "tests/fixtures/fake-pi-sdk"
# Valores plantados na fixture: nenhum pode voltar do adaptador.
PLANTED = ("FIXTURE", "sk-ant-", "sk-live-", "eyJhbGciOi", "refresh-token", "access-token")

requires_node = unittest.skipUnless(shutil.which("node"), "node is required for the adapter")


class AdapterSession:
    """Uma conversa JSONL com o adaptador, com leitura em thread própria."""

    def __init__(self, env=None, package=FIXTURE):
        merged = dict(os.environ)
        merged.pop("DISH_PI_PACKAGE", None)
        merged.pop("FAKE_PI_EMPTY", None)
        merged.pop("FAKE_PI_LOGIN_FAIL", None)
        if env:
            merged.update(env)
        self.stderr = tempfile.TemporaryFile(mode="w+")
        self.process = subprocess.Popen(
            ["node", str(ADAPTER), "--package", str(package)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            text=True, encoding="utf-8", env=merged)
        self.lines = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()

    def _reader(self):
        for line in self.process.stdout:
            line = line.strip()
            if line:
                self.lines.put(json.loads(line))
        self.lines.put(None)

    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def next_line(self, timeout=30):
        try:
            message = self.lines.get(timeout=timeout)
        except queue.Empty:
            self.fail_with_stderr("o adaptador não respondeu")
        if message is None:
            self.fail_with_stderr("o adaptador encerrou a saída")
        return message

    def fail_with_stderr(self, reason):
        self.stderr.seek(0)
        raise AssertionError(f"{reason}: {self.stderr.read()}")

    def next_for(self, op_id, timeout=30):
        """Próxima mensagem da operação `op_id`, pulando outras."""
        deadline = threading.Event()
        while True:
            message = self.next_line(timeout=timeout)
            if message.get("id") == op_id or message.get("id") is None:
                return message
            if deadline.wait(0):
                return message

    def close(self):
        try:
            self.process.stdin.close()
            self.process.wait(timeout=10)
        except Exception:
            self.process.kill()
        for stream in (self.process.stdout, self.stderr):
            try:
                stream.close()
            except Exception:
                pass


@requires_node
class ProviderLoginTests(unittest.TestCase):
    def session(self, env=None):
        session = AdapterSession(env=env)
        self.addCleanup(session.close)
        return session

    def configured(self, session, op_id):
        session.send({"id": op_id, "op": "status"})
        while True:
            message = session.next_line()
            if message.get("id") == op_id:
                break
        self.assertTrue(message["ok"], message)
        return {p["id"]: p["configured"] for p in message["data"]["providers"]}

    def test_api_key_login_asks_for_a_secret_and_never_returns_it(self):
        session = self.session({"FAKE_PI_EMPTY": "1"})
        self.assertFalse(self.configured(session, 1)["anthropic"])
        session.send({"id": 2, "op": "login", "provider": "anthropic", "type": "api_key"})
        ask = session.next_line()
        self.assertEqual(ask["id"], 2)
        self.assertEqual(ask["ask"], "secret")
        self.assertEqual(ask["message"], "Cole a chave de API da Anthropic")
        self.assertIn("prompt", ask)
        # A resposta vai, o valor não volta.
        session.send({"id": 3, "op": "reply", "prompt": ask["prompt"], "value": "sk-ant-chave-de-teste"})
        result = session.next_line()
        self.assertEqual(result["id"], 2)
        self.assertTrue(result["ok"], result)
        self.assertEqual(result["data"], {"provider": "anthropic", "type": "api_key"})
        self.assertNotIn("chave-de-teste", json.dumps(result))
        self.assertTrue(self.configured(session, 4)["anthropic"])

    def test_oauth_login_walks_through_url_device_code_and_manual_code(self):
        session = self.session({"FAKE_PI_EMPTY": "1"})
        session.send({"id": 1, "op": "login", "provider": "openai", "type": "oauth"})
        seen = {}
        ask = None
        while ask is None:
            message = session.next_line()
            if message["id"] != 1:
                continue
            if message.get("notify"):
                seen.setdefault(message["notify"], message)
                continue
            if message.get("ask"):
                ask = message
        # Os avisos chegam antes do pedido do código, não no lugar dele.
        self.assertEqual(ask["ask"], "manual_code")
        self.assertEqual(ask["message"], "Cole o código de autorização")
        self.assertEqual(seen["auth_url"]["url"], "https://example.invalid/authorize")
        self.assertEqual(seen["device_code"]["userCode"], "ABCD-1234")
        self.assertEqual(seen["device_code"]["verificationUri"], "https://example.invalid/device")
        self.assertEqual(seen["device_code"]["intervalSeconds"], 5)
        self.assertEqual(seen["device_code"]["expiresInSeconds"], 900)
        self.assertEqual(seen["progress"]["message"], "Aguardando a autorização…")
        self.assertEqual(seen["info"]["links"], [{"url": "https://example.invalid/help", "label": "Ajuda"}])
        session.send({"id": 2, "op": "reply", "prompt": ask["prompt"], "value": "123456"})
        result = session.next_line()
        self.assertTrue(result["ok"], result)
        self.assertEqual(result["data"], {"provider": "openai", "type": "oauth"})
        for planted in PLANTED:
            self.assertNotIn(planted, json.dumps(result))
        self.assertTrue(self.configured(session, 3)["openai"])

    def test_select_prompt_carries_options_and_text_prompt_gets_the_value(self):
        session = self.session()
        session.send({"id": 1, "op": "login", "provider": "select-provider", "type": "api_key"})
        ask = session.next_line()
        self.assertEqual(ask["ask"], "select")
        self.assertEqual(
            ask["options"],
            [{"id": "br", "label": "Brasil", "description": "southamerica-east1"},
             {"id": "us", "label": "Estados Unidos", "description": "us-east-1"}])
        session.send({"id": 2, "op": "reply", "prompt": ask["prompt"], "value": "br"})
        follow = session.next_line()
        self.assertEqual(follow["ask"], "text")
        self.assertEqual(follow["message"], "Cole a chave")
        session.send({"id": 3, "op": "reply", "prompt": follow["prompt"], "value": "chave-de-teste"})
        result = session.next_line()
        self.assertTrue(result["ok"], result)
        self.assertNotIn("chave-de-teste", json.dumps(result))

    def test_logout_removes_the_credential(self):
        session = self.session()
        self.assertTrue(self.configured(session, 1)["openai"])
        session.send({"id": 2, "op": "logout", "provider": "openai"})
        result = session.next_line()
        self.assertEqual(result["id"], 2)
        self.assertTrue(result["ok"], result)
        self.assertEqual(result["data"], {"provider": "openai"})
        self.assertFalse(self.configured(session, 3)["openai"])

    def test_a_failed_login_hides_the_key_from_the_error(self):
        session = self.session({"FAKE_PI_EMPTY": "1", "FAKE_PI_LOGIN_FAIL": "1"})
        session.send({"id": 1, "op": "login", "provider": "anthropic", "type": "api_key"})
        ask = session.next_line()
        # A fixture devolve a própria chave na mensagem de erro: ela não pode sair.
        session.send({"id": 2, "op": "reply", "prompt": ask["prompt"], "value": "sk-ant-chave-recusada"})
        result = session.next_line()
        self.assertEqual(result["id"], 1)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "login_failed")
        self.assertNotIn("chave-recusada", json.dumps(result))
        for planted in PLANTED:
            self.assertNotIn(planted, json.dumps(result))

    def test_an_unsupported_type_is_reported_as_such(self):
        session = self.session({"FAKE_PI_EMPTY": "1"})
        session.send({"id": 1, "op": "login", "provider": "anthropic", "type": "oauth"})
        result = session.next_line()
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "unsupported_type")
        self.assertTrue(self.configured(session, 2)["anthropic"] is False)

    def test_cancel_stops_a_pending_login(self):
        session = self.session({"FAKE_PI_EMPTY": "1"})
        session.send({"id": 1, "op": "login", "provider": "anthropic", "type": "api_key"})
        session.next_line()
        session.send({"id": 2, "op": "cancel", "target": 1})
        result = session.next_line()
        self.assertEqual(result["id"], 1)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "cancelled")
        # O adaptador continua utilizável depois de cancelar.
        self.assertFalse(self.configured(session, 3)["anthropic"])

    def test_only_one_login_at_a_time(self):
        session = self.session({"FAKE_PI_EMPTY": "1"})
        session.send({"id": 1, "op": "login", "provider": "anthropic", "type": "api_key"})
        session.next_line()
        session.send({"id": 2, "op": "login", "provider": "openai", "type": "api_key"})
        result = session.next_line()
        self.assertEqual(result["id"], 2)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "busy")
        session.send({"id": 3, "op": "cancel", "target": 1})
        session.next_line()

    def test_reply_without_a_pending_prompt_is_refused(self):
        session = self.session()
        session.send({"id": 1, "op": "reply", "prompt": 99, "value": "x"})
        result = session.next_line()
        self.assertEqual(result["error"], "unknown_prompt")


if __name__ == "__main__":
    unittest.main()
