"""Xvfb UI test: ordem das sessões/projetos, agrupamento e persistência.

Duas pastas com uma sessão salva cada. O padrão ordena os projetos por
atividade (zeta-proj antes de alfa-proj); a seção Navegação troca para ordem
por nome e lista única. O restart preserva as escolhas, e valores inválidos no
state.json voltam ao padrão sem quebrar a lista. Tudo é lido do probe
`nav-list`; nenhuma verificação depende de OCR.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-nav-order-") as temporary:
    temp = Path(temporary)
    alfa = temp / "alfa-proj"
    zeta = temp / "zeta-proj"
    alfa.mkdir()
    zeta.mkdir()
    (temp / "home").mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    now = time.time()
    for name, project, age in (("alfa-velha", alfa, 7200), ("zeta-nova", zeta, 60)):
        path = sessions / f"{name}.jsonl"
        path.write_text(json.dumps({"type": "session", "cwd": str(project), "id": name}) + "\n"
                        + json.dumps({"type": "session_info", "name": name}) + "\n")
        os.utime(path, (now - age, now - age))
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    state = config / "state.json"
    state.write_text(json.dumps({"appearance": "dark"}))
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(temp / "events.jsonl"),
               DISH_TEST_UI_LOG=str(temp / "ui-1.jsonl"), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")

    def launch(ui_log):
        return subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(zeta)],
            env=dict(env, DISH_TEST_UI_LOG=str(ui_log)), stdout=log, stderr=log)

    def saved_state():
        # A escrita é atômica; uma leitura no meio pode ver sumir o arquivo.
        try:
            return json.loads(state.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def events():
        path = temp / "events.jsonl"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines() if line]

    def get_messages_count():
        return sum(1 for entry in events() if entry.get("command", {}).get("type") == "get_messages")

    window = None

    def nav():
        if not window:
            return None
        node = UI_TRACES[window].node("nav-list")
        return node["data"] if node else None

    def segment_active(identity):
        node = UI_TRACES[window].node(identity) if window else None
        return bool(node and node["data"].get("active"))

    def project_names(data):
        return [Path(path).name for path in data["projects"]]

    def comes_before(values, first, second):
        return first in values and second in values and values.index(first) < values.index(second)

    def leave_rail(current):
        """Tira o ponteiro do rail: com hover a lista congela a ordem anterior."""
        root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        x11.XTranslateCoordinates(display, current, root_window, 0, 0,
                                  ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
        xtst.XTestFakeMotionEvent(display, -1, root_x.value + 620, root_y.value + 420, 0)
        x11.XFlush(display)
        time.sleep(0.4)

    def wait_ready(after, ui_log):
        wait_for(lambda: find_window(), "Janela do Dish não foi criada")
        current = find_window()
        UI_TRACES[current] = UiTrace(ui_log)
        wait_for(lambda: get_messages_count() > after,
                 "Fake Pi não inicializou a sessão", timeout=30)
        x11.XSetInputFocus(display, current, 1, 0)
        expose_window(current)
        wait_for(lambda: UI_TRACES[current].node("nav-list") is not None,
                 "Lista de navegação não foi pintada", timeout=30)
        return current

    def open_navigation_settings():
        press("F1")  # abre as Configurações na seção Atalhos de teclado
        # Único clique por OCR (o rótulo estático do menu); prefixo sem acento porque o OCR nem sempre reconhece ç/ã. Todas as verificações seguem pelo probe.
        def target():
            return find_word(window, "Naveg")
        x, y, width, height = wait_for(target, "Item Navegação do menu não apareceu", timeout=30)
        click(window, x + width // 2, y + height // 2)
        wait_ui(window, "nav-projects-name", "Seção Navegação não abriu")

    def close_settings():
        press("Escape")
        wait_for(lambda: UI_TRACES[window].node("nav-projects-name") is None,
                 "Escape não fechou as configurações", timeout=20)

    def stop():
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        log.flush()

    def default_activity_order():
        data = nav()
        return bool(data and data["layout"] == "projects"
                    and comes_before(project_names(data), "zeta-proj", "alfa-proj")
                    and "alfa-velha" in data["titles"] and "zeta-nova" in data["titles"])

    def projects_by_name():
        data = nav()
        return bool(data and data["layout"] == "projects"
                    and comes_before(project_names(data), "alfa-proj", "zeta-proj"))

    def flat_by_name():
        data = nav()
        return bool(data and data["layout"] == "flat"
                    and comes_before(data["titles"], "alfa-velha", "zeta-nova"))

    process = launch(temp / "ui-1.jsonl")
    try:
        # Passo 1: padrão — projetos por atividade, lista agrupada.
        window = wait_ready(0, temp / "ui-1.jsonl")
        leave_rail(window)
        wait_for(default_activity_order,
                 "Padrão por atividade não colocou zeta-proj antes de alfa-proj", timeout=30)

        # Passo 2: ordem dos projetos por nome.
        open_navigation_settings()
        click_ui(window, "nav-projects-name")
        wait_for(lambda: segment_active("nav-projects-name"),
                 "Segmento de ordem por nome não ficou ativo", timeout=20)
        close_settings()
        leave_rail(window)
        wait_for(projects_by_name,
                 "Ordem por nome não colocou alfa-proj antes de zeta-proj", timeout=30)
        wait_for(lambda: saved_state().get("project_order") == "name",
                 "project_order=name não foi gravado no state.json", timeout=20)

        # Passo 3: lista única e sessões por nome.
        open_navigation_settings()
        click_ui(window, "nav-layout-flat")
        wait_for(lambda: segment_active("nav-layout-flat"),
                 "Segmento de lista única não ficou ativo", timeout=20)
        click_ui(window, "nav-order-name")
        wait_for(lambda: segment_active("nav-order-name"),
                 "Segmento de ordem por nome não ficou ativo", timeout=20)
        close_settings()
        leave_rail(window)
        wait_for(flat_by_name,
                 "Lista única por nome não colocou alfa-velha antes de zeta-nova", timeout=30)

        def flat_name_saved():
            data = saved_state()
            return data.get("session_layout") == "flat" and data.get("session_order") == "name"

        wait_for(flat_name_saved,
                 "session_layout=flat e session_order=name não foram gravados", timeout=20)

        # Passo 4: as escolhas sobrevivem ao restart.
        before = get_messages_count()
        stop()
        process = launch(temp / "ui-2.jsonl")
        window = wait_ready(before, temp / "ui-2.jsonl")
        leave_rail(window)
        wait_for(flat_by_name,
                 "Lista única por nome não sobreviveu ao restart", timeout=30)

        # Passo 5: valores inválidos caem no padrão sem quebrar a lista.
        before = get_messages_count()
        stop()
        state.write_text(json.dumps({"appearance": "dark", "session_layout": "bogus",
                                     "project_order": 7}))
        process = launch(temp / "ui-3.jsonl")
        window = wait_ready(before, temp / "ui-3.jsonl")
        leave_rail(window)
        wait_for(default_activity_order,
                 "Valores inválidos não voltaram ao padrão por atividade", timeout=30)
        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Navigation order smoke passed")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-nav-order-failure-", suffix=".png")
        subprocess.run(["import", "-window", str(window) if window else "root", screenshot], check=False)
        print("Navigation order failure screenshot:", screenshot)
        if window:
            save_ui_trace(window, "dish-nav-order")
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
