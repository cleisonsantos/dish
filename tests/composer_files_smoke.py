"""Anexos de arquivo: colagem, seletor, remoção e envio (#172)."""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-composer-files-") as temporary:
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

    notes = temp / "notes.txt"
    notes.write_bytes(b"a" * 1536)
    second = temp / "second.txt"
    second.write_text("segundo")
    blob = temp / "blob.bin"
    blob.write_bytes(b"\x00\x01texto")
    big = temp / "big.txt"
    with big.open("wb") as fixture:
        fixture.truncate(11 * 1024 * 1024)
    gone = temp / "gone.txt"
    gone.write_text("vai sumir")
    # O processo recebe o gancho antes de iniciar.
    env["DISH_TEST_PICK_PATHS"] = str(second)

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

        def clipboard(data, mime):
            holder = subprocess.Popen(["xclip", "-selection", "clipboard", "-in", "-target", mime, "-quiet"],
                                      stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            holder.stdin.write(data)
            holder.stdin.close()
            end = time.monotonic() + 5
            while time.monotonic() < end:
                probe = subprocess.run(["xclip", "-selection", "clipboard", "-out", "-target", mime],
                                       capture_output=True, timeout=5)
                if probe.stdout:
                    break
                time.sleep(0.1)
            return holder

        def probe(condition, description):
            return wait_probe("composer-input", description,
                              condition=lambda node: condition(node["data"]))["data"]

        def paste_file(path, condition):
            holder = clipboard((path.as_uri() + "\r\n").encode(), "text/uri-list")
            try:
                click_ui(window, "composer-input")
                chord("Control_L", "v")
                return probe(condition, "A colagem não produziu o estado esperado")
            finally:
                holder.terminate()
                holder.wait(timeout=5)

        def toast(fragment):
            return lambda data: any(fragment in text for text in data["toasts"])

        def prompts():
            return [event["command"] for event in events()
                    if event.get("command", {}).get("type") == "prompt"]

        probe(lambda data: not data["loading"], "Composer não iniciou")
        data = paste_file(notes, lambda data: len(data["attachments"]) == 1)
        assert data["attachments"] == [{"kind": "file", "name": "notes.txt", "size": "1,5 KB", "missing": False}]
        assert data["stats"]["chars"] == 0
        data = paste_file(notes, toast("já está anexado"))
        assert len(data["attachments"]) == 1
        data = paste_file(blob, toast("parece binário"))
        assert len(data["attachments"]) == 1
        data = paste_file(big, toast("limite por arquivo é 10 MB"))
        assert len(data["attachments"]) == 1
        click_ui(window, "attach-button")
        probe(lambda data: len(data["attachments"]) == 2, "Seletor não anexou second.txt")
        click_ui(window, "remove-attached-file:second.txt")
        probe(lambda data: len(data["attachments"]) == 1, "Remoção não preservou notes.txt")
        click_ui(window, "composer-input")
        type_text("leia")
        press("Return")
        wait_for(lambda: prompts(), "Prompt não chegou ao Pi")
        assert prompts()[-1]["message"] == 'leia\n\nArquivos anexados:\n- "' + str(notes.resolve()) + '"'
        assert "images" not in prompts()[-1]
        probe(lambda data: data["attachments"] == [], "Anexos não foram limpos")
        paste_file(gone, lambda data: len(data["attachments"]) == 1)
        gone.unlink()
        type_text("leia ausente")
        before = len(prompts())
        press("Return")
        data = probe(lambda data: data["attachments"][0]["missing"], "Arquivo ausente não foi marcado")
        assert data["stats"]["chars"] > 0
        assert any("não existe mais no disco" in text for text in data["toasts"])
        time.sleep(2)
        assert len(prompts()) == before
        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Composer files smoke passed")
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
