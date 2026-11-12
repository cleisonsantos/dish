"""MCP stdio e registro: stdlib, dados sintéticos, nenhuma janela."""
import hashlib
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("DISH_TEST_BIN", ROOT / "target/debug/dish")).resolve()
FIELDS = set("details_open navigation_open show_thinking generated_titles completion_sound hidden_models pinned_sessions session_order session_layout project_order appearance reduce_motion theme pi_auto_check pi_auto_install update_policy".split())


class McpTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="dish-mcp-test-")
        self.addCleanup(self.tmp.cleanup)
        self.home = Path(self.tmp.name)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(("PI_", "DISH_", "XDG_")) and key not in ("DISPLAY", "WAYLAND_DISPLAY")}
        self.env.update(HOME=str(self.home), XDG_CONFIG_HOME=str(self.home / "config"),
                        XDG_CACHE_HOME=str(self.home / "cache"), XDG_DATA_HOME=str(self.home / "data"),
                        XDG_STATE_HOME=str(self.home / "state"), XDG_RUNTIME_DIR=str(self.home / "runtime"))
        # Nenhum Node: exercita a indisponibilidade sem consultar dados reais.
        self.env["PATH"] = str(self.home / "empty-bin")

    def command(self, *args):
        return subprocess.run([str(BIN), *args], env=self.env, cwd=self.home,
                              capture_output=True, text=True, timeout=10)

    def test_stdio_read_only(self):
        state = self.home / "config/dish/state.json"
        state.parent.mkdir(parents=True)
        state.write_text(json.dumps({"generated_titles": False, "navigation_width": 987,
                                     "last_update_etag": "not-exported", "future_field": "not-exported"}))
        before = state.read_bytes(), state.stat().st_mtime_ns
        sessions = self.home / ".pi/agent/sessions/project"
        sessions.mkdir(parents=True)
        for index in range(3):
            (sessions / f"session{index}.jsonl").write_text(
                json.dumps({"type": "session", "id": str(index), "cwd": "/synthetic/project"}) + "\n" +
                json.dumps({"type": "session_info", "name": f"Synthetic {index}"}) + "\n")
        process = subprocess.Popen([str(BIN), "--mcp-serve"], env=self.env, cwd=self.home,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        selector = selectors.DefaultSelector()
        self.addCleanup(selector.close)
        selector.register(process.stdout, selectors.EVENT_READ)

        def read():
            self.assertTrue(selector.select(timeout=10), "servidor não respondeu")
            line = process.stdout.readline()
            self.assertTrue(line, "servidor encerrou")
            return json.loads(line)

        def send(value):
            process.stdin.write((json.dumps(value) + "\n").encode())
            process.stdin.flush()

        request_id = 0

        def rpc(method, params=None):
            nonlocal request_id
            request_id += 1
            value = {"jsonrpc": "2.0", "id": request_id, "method": method}
            if params is not None:
                value["params"] = params
            send(value)
            response = read()
            self.assertEqual(response["id"], request_id)
            self.assertEqual(response["jsonrpc"], "2.0")
            self.assertNotIn("error", response)
            return response["result"]

        def call(name, args=None):
            return rpc("tools/call", {"name": name, "arguments": args or {}})

        def data(name, args=None):
            result = call(name, args)
            self.assertFalse(result["isError"])
            value = json.loads(result["content"][0]["text"])
            self.assertEqual(value["source"], "disk")
            self.assertIs(value["gui"], False)
            return value["data"]

        self.assertEqual(rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                          "clientInfo": {"name": "test", "version": "1"}})["protocolVersion"], "2024-11-05")
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        listing = rpc("tools/list")["tools"]
        self.assertEqual({tool["name"] for tool in listing},
                         {"dish_status", "dish_preferences", "dish_sessions", "dish_providers", "dish_settings_sections"})
        for tool in listing:
            self.assertIs(tool["annotations"]["readOnlyHint"], True)
            schema = json.dumps(tool["inputSchema"]).lower()
            for term in ("key", "token", "secret", "password", "auth"):
                self.assertNotIn(term, schema)
        prefs = data("dish_preferences")
        self.assertEqual(set(prefs), FIELDS)
        self.assertIs(prefs["generated_titles"], False)
        self.assertNotIn("not-exported", json.dumps(prefs))
        status = data("dish_status")
        self.assertIs(status["gui"], False)
        self.assertIsNone(status["session"])
        self.assertEqual(len(data("dish_sessions")["sessions"]), 3)
        limited = data("dish_sessions", {"cwd": "/synthetic/project", "limit": 2})["sessions"]
        self.assertEqual(len(limited), 2)
        self.assertTrue(all(not session["open"] and not session["running"] for session in limited))
        self.assertEqual(data("dish_sessions", {"cwd": "/elsewhere"})["sessions"], [])
        self.assertEqual(len(data("dish_settings_sections")["sections"]), 8)
        providers = call("dish_providers")
        self.assertIs(providers["isError"], True)
        self.assertEqual(providers["content"][0]["text"], "Não foi possível consultar os provedores do Pi.")
        for name, args in [("dish_sessions", {"limit": 501}), ("dish_preferences", {"extra": 1}),
                           ("dish_open_settings", {})]:
            self.assertIs(call(name, args)["isError"], True)
        process.stdin.write(b"invalid-json\n")
        process.stdin.flush()
        self.assertEqual(read()["error"]["code"], -32700)
        self.assertEqual(set(data("dish_preferences")), FIELDS)  # processo sobrevive ao erro
        process.stdin.close()
        self.assertEqual(process.wait(timeout=2), 0)
        self.assertEqual(process.stderr.read(), b"")
        process.stdout.close()
        process.stderr.close()
        self.assertEqual((state.read_bytes(), state.stat().st_mtime_ns), before)

    def test_provider_projection_excludes_unlisted_fields(self):
        node = self.home / "empty-bin/node"
        node.parent.mkdir()
        payload = {"ok": True, "data": {"pi": {"version": "1.5.0"}, "providers": [
            {"id": "synthetic", "models": 3, "configured": True, "source": "stored",
             "future_field": "never-export", "auth": {"api_key": {"name": "never-export"},
                                                         "oauth": {"name": "never-export"}}}]}}
        node.write_text("#!" + sys.executable + "\nprint(" + repr(json.dumps(payload)) + ")\n")
        node.chmod(0o700)
        requests = [{"jsonrpc": "2.0", "id": index, "method": "tools/call",
                     "params": {"name": name, "arguments": {}}}
                    for index, name in enumerate(("dish_providers", "dish_status"), 1)]
        result = subprocess.run([str(BIN), "--mcp-serve"], env=self.env, cwd=self.home,
                                input="".join(json.dumps(request) + "\n" for request in requests),
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        responses = [json.loads(line)["result"] for line in result.stdout.splitlines()]
        self.assertEqual(len(responses), 2)
        data = [json.loads(response["content"][0]["text"])["data"] for response in responses]
        self.assertEqual(data[0]["providers"], [{"id": "synthetic", "models": 3,
                                               "configured": True, "source": "stored"}])
        self.assertEqual(data[1]["pi_version"], "1.5.0")
        self.assertNotIn("never-export", result.stdout)

    def test_registry_ownership_and_idempotence(self):
        path = self.home / ".pi/agent/mcp.json"
        path.parent.mkdir(parents=True)
        foreign = b'{ "mcpServers": {"dish": {"command":"foreign"}} }\n'
        path.write_bytes(foreign)
        before = hashlib.sha256(path.read_bytes()).hexdigest()
        result = self.command("--install-mcp")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), before)
        self.assertEqual(self.command("--uninstall-mcp").returncode, 0)
        self.assertEqual(path.read_bytes(), foreign)
        path.write_text('{"mcpServers":{"other":{"command":"other"}},"extra":true}')
        self.assertEqual(self.command("--install-mcp").returncode, 0)
        first = path.read_bytes(), path.stat().st_mtime_ns
        self.assertEqual(self.command("--install-mcp").returncode, 0)
        self.assertEqual((path.read_bytes(), path.stat().st_mtime_ns), first)
        self.assertEqual(json.loads(path.read_bytes())["mcpServers"]["dish"]["x-dish-managed"], "176")
        self.assertEqual(self.command("--uninstall-mcp").returncode, 0)
        self.assertEqual(json.loads(path.read_bytes()), {"mcpServers": {"other": {"command": "other"}}, "extra": True})
        first = path.read_bytes(), path.stat().st_mtime_ns
        self.assertEqual(self.command("--uninstall-mcp").returncode, 0)
        self.assertEqual((path.read_bytes(), path.stat().st_mtime_ns), first)
        self.assertEqual(self.command("--install-mcp", "--project").returncode, 0)
        self.assertTrue((self.home / ".pi/mcp.json").exists())
        self.assertNotEqual(self.command("--model", "test", "--mcp-serve").returncode, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
