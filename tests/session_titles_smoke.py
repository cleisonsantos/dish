"""Xvfb regression: semantic titles, Pi persistence, manual names and failures.
All homes, sessions, logs and model responses are synthetic; no network.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-titles-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    (temp / "home").mkdir()
    session = temp / "session.jsonl"
    session.write_text(json.dumps({"type": "session", "id": "synthetic-title-session", "cwd": str(project)}) + "\n")
    config = temp / "config/dish"
    config.mkdir(parents=True)
    (config / "state.json").write_text(json.dumps({"generated_titles": True, "appearance": "dark"}))
    wrapper = temp / "pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/title_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    records = temp / "commands.jsonl"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), XDG_SESSION_TYPE="x11", DISH_TITLE_LOG=str(records))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    process = None
    output = open(temp / "dish.log", "w")

    def events():
        result = []
        for line in records.read_text().splitlines() if records.exists() else []:
            try:
                result.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return result

    def commands(kind):
        return [event["command"] for event in events() if event.get("command", {}).get("type") == kind]

    def generations():
        return [event for event in events() if "generation" in event]

    def start():
        global process
        before = len(commands("get_messages"))
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")),
                                    "--session", str(session), str(project)], env=env, stdout=output, stderr=output)
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: len(commands("get_messages")) > before, "Session not loaded")
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        chord("Control_L", "l")
        return window

    def stop():
        global process
        if process:
            process.terminate()
            process.wait(timeout=10)
            process = None

    def send(text):
        chord("Control_L", "l")
        type_text(text)
        press("Return")

    try:
        window = start()
        send("Eu queria que voce corrigisse os nomes das conversas")
        wait_for(lambda: commands("set_session_name"), "Semantic title not sent to Pi")
        assert commands("set_session_name")[-1]["name"] == "Corrigir nomes das conversas"
        wait_for(lambda: "Corrigir nomes das conversas" in session.read_text(), "Title not persisted")
        send("Agora exporte um relatorio de todas as alteracoes")
        wait_for(lambda: len(commands("prompt")) == 2, "Follow-up not delivered")
        stop()
        assert len(commands("set_session_name")) == 1, "Named session was renamed"
        window = start()
        wait_for(lambda: find_word(window, "Corrigir"), "Persisted title not displayed after restart")
        send("Mais um pedido que nao deve substituir o nome")
        wait_for(lambda: len(commands("prompt")) == 3, "Reopened prompt not delivered")
        stop()
        assert len(generations()) == 1, "Reopened named session generated a new title"

        # A manual rename while generation is in flight wins over the late result.
        session.write_text(json.dumps({"type": "session", "id": "synthetic-title-session", "cwd": str(project)}) + "\n")
        gate = temp / "release-title"
        env["DISH_TITLE_GATE"] = str(gate)
        window = start()
        send("Corrija os nomes das sessoes com resumo semantico")
        wait_for(lambda: len(generations()) == 2, "Delayed generation did not start")
        send("/name Nome manual preservado")
        press("Return")
        wait_for(lambda: "nome manual preservado" in session.read_text().lower(), "Manual name not saved")
        gate.touch()
        send("Pedido adicional para confirmar que o nome manual fica")
        wait_for(lambda: len(commands("prompt")) == 5, "Final prompt not delivered")
        stop()
        assert len(commands("set_session_name")) == 2, "Late generated title overwrote manual name"
        assert "nome manual preservado" in session.read_text().lower()

        session.write_text(json.dumps({"type": "session", "id": "synthetic-title-session", "cwd": str(project)}) + "\n")
        env.pop("DISH_TITLE_GATE")
        env["DISH_TITLE_FAIL"] = "1"
        window = start()
        send("Corrija os nomes das sessoes com resumo semantico")
        wait_for(lambda: find_word(window, "gerar"), "Generation failure was silent")
        assert len(commands("set_session_name")) == 2, "Failure persisted a literal fallback"
        assert "synthetic private provider diagnostic" not in (temp / "dish.log").read_text()
        print("Session title smoke passed")
    except Exception:
        print(json.dumps(events(), indent=2))
        raise
    finally:
        stop()
        output.close()
