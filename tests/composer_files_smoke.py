"""Anexos de arquivo: validação, chip, remoção e envio (#172).

A entrada usada aqui é o botão Anexar através do gancho `DISH_TEST_PICK_PATHS`,
que entrega os caminhos ao mesmo `attach_paths` usado por colar e soltar. A
colagem em si não é simulada porque o backend do GPUI no Linux não serializa
`text/uri-list` no ambiente do CI; o caminho de imagem colada continua coberto
por `desktop_smoke.py`, e o de arquivo pela mesma `attach_paths` exercitada aqui.
"""
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
    pick_list = temp / "pick.txt"
    pick_list.write_text(str(second))
    env["DISH_TEST_PICK_PATHS"] = str(pick_list)


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

        def attach(*paths):
            """Serve os caminhos ao botão Anexar; o gancho é relido a cada clique."""
            # `attach_paths` ignora clique novo enquanto a ingestão anterior está
            # em andamento; esperar o fim evita perder o anexo por corrida — era
            # assim que este smoke falhava de vez em quando no CI.
            wait_probe("composer-input", "Anexo anterior não terminou",
                       lambda node: not node["data"]["loading"])
            pick_list.write_text("\n".join(str(path) for path in paths))

            def estado():
                expose_window(window)
                node = UI_TRACES[window].node("composer-input")
                return json.dumps(node["data"], sort_keys=True) if node else ""

            antes = estado()
            for tentativa in (1, 2):
                click_ui(window, "attach-button")
                # Um clique perdido (janela sem foco no instante do evento)
                # deixaria o passo seguinte esperando para sempre: refaz uma vez.
                fim = time.monotonic() + 3
                while time.monotonic() < fim:
                    if estado() != antes:
                        return
                    time.sleep(0.05)
                if tentativa == 2:
                    raise AssertionError(
                        f"O clique em Anexar não teve efeito; probe={antes}")

        def probe(condition, description, timeout=10):
            def pronto():
                expose_window(window)
                node = UI_TRACES[window].node("composer-input")
                return node and visible_point(node) and condition(node["data"])
            wait_for(pronto, description, timeout=timeout)
            return UI_TRACES[window].node("composer-input")["data"]

        def attach_and_wait(paths, condition):
            attach(*paths)
            try:
                # A ingestão é IO no executor de background; num runner carregado
                # ela pode demorar bem mais que os 10 s padrão.
                return probe(condition, "O anexo não produziu o estado esperado", timeout=30)
            except AssertionError:
                node = UI_TRACES[window].node("composer-input")
                raise AssertionError(
                    f"O anexo {[path.name for path in paths]} não produziu o estado esperado; "
                    f"probe={json.dumps(node['data']) if node else 'ausente'}"
                )

        def toast(fragment):
            return lambda data: any(fragment in text for text in data["toasts"])

        def prompts():
            return [event["command"] for event in events()
                    if event.get("command", {}).get("type") == "prompt"]

        probe(lambda data: not data["loading"], "Composer não iniciou")
        data = attach_and_wait([notes], lambda data: len(data["attachments"]) == 1)
        assert data["attachments"] == [{"kind": "file", "name": "notes.txt", "size": "1,5 KB", "missing": False}]
        assert data["stats"]["chars"] == 0
        data = attach_and_wait([notes], toast("já está anexado"))
        assert len(data["attachments"]) == 1
        data = attach_and_wait([blob], toast("parece binário"))
        assert len(data["attachments"]) == 1
        data = attach_and_wait([big], toast("limite por arquivo é 10 MB"))
        assert len(data["attachments"]) == 1
        attach_and_wait([second], lambda data: len(data["attachments"]) == 2)
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
        attach_and_wait([gone], lambda data: len(data["attachments"]) == 1)
        gone.unlink()
        click_ui(window, "composer-input")
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
        print("PATH:", env.get("PATH"))
        print("shim chamado:", shim_log.read_text() if shim_log.exists() else "NAO")
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
