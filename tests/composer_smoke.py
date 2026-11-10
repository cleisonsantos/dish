"""Composer expandido: alt-shift-e, Esc e envio (#172)."""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-composer-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    (temp / "home").mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    archive = sessions / "archive.jsonl"
    records = [{"type": "session", "cwd": str(project), "id": "synthetic-composer"},
               {"type": "session_info", "name": "Composer expanded"},
               {"type": "message", "message": {"role": "assistant", "content": [
                   {"type": "text", "text": "composer smoke ready"}]}}]
    archive.write_text("\n".join(json.dumps(record) for record in records) + "\n")
    gates = temp / "stream-gates"
    gates.mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(log_path),
               DISH_FAKE_PI_STREAM_GATE_DIR=str(gates), XDG_SESSION_TYPE="x11",
               DISH_TEST_UI_LOG=str(temp / "ui.jsonl"))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    # A aparência segue o desktop por padrão; estes testes verificam o visual
    # escuro, então a preferência fica fixada como numa instalação existente.
    (temp / "config/dish").mkdir(parents=True, exist_ok=True)
    (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))

    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(project)],
                               env=env, stdout=log, stderr=log)

    def events():
        return read_events(log_path)

    window = None
    try:
        wait_for(lambda: find_window(), "Composer smoke window not created")
        window = find_window()
        UI_TRACES[window] = UiTrace(temp / "ui.jsonl")
        wait_for(lambda: any(event.get("command", {}).get("type") == "get_messages" for event in events()),
                 "Pi did not initialize")
        x11.XSetInputFocus(display, window, 1, 0)

        def wait_probe(identity, description, condition=lambda node: True):
            """Espera um probe pintado; expõe a janela a cada poll.

            Xvfb não tem WM: sem `expose_window` repetido a janela não recebe o
            primeiro frame e nenhum probe chega ao trace.
            """
            def ready():
                expose_window(window)
                node = UI_TRACES[window].node(identity)
                return node and visible_point(node) and condition(node)
            wait_for(ready, description)
            return UI_TRACES[window].node(identity)

        # 1. O composer começa na altura normal.
        node = wait_probe("composer-input", "Composer did not render")
        assert node["data"]["expanded"] is False, \
            f"Composer started expanded: {node['data']!r}"

        # 2. alt-shift-e expande com o foco no editor.
        click_ui(window, "composer-input")
        chord("Alt_L", "Shift_L", "e")
        wait_probe("composer-input", "Alt+Shift+E did not expand the composer",
                condition=lambda node: node["data"]["expanded"] is True)

        # 3. Esc desmonta o expandido sem descartar o rascunho.
        type_text("rascunho-172")
        wait_probe("frame", "The draft did not reach the composer",
                condition=lambda node: node["data"]["draft"] == "rascunho-172")
        press("Escape")
        wait_probe("composer-input", "Escape did not collapse the composer",
                condition=lambda node: node["data"]["expanded"] is False)
        wait_probe("frame", "Escape discarded the draft",
                condition=lambda node: node["data"]["draft"] == "rascunho-172")

        # 4. Enviar também volta ao normal.
        chord("Alt_L", "Shift_L", "e")
        wait_probe("composer-input", "Alt+Shift+E did not expand the composer again",
                condition=lambda node: node["data"]["expanded"] is True)
        press("Return")
        wait_probe("composer-input", "Send did not collapse the composer",
                condition=lambda node: node["data"]["expanded"] is False)

        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Composer expanded smoke passed")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-composer-failure-", suffix=".png")
        subprocess.run(["import", "-window", str(window) if window else "root", screenshot], check=False)
        print("Composer expanded failure screenshot:", screenshot)
        if window:
            save_ui_trace(window, "dish-composer")
        print(log_path.read_text() if log_path.exists() else "No fixture events")
        print((temp / "dish.log").read_text())
        raise
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        log.close()
