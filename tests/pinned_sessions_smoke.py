"""Xvfb UI test: fixar uma sessão no topo do grupo e sobreviver ao restart.

Duas sessões salvas na mesma pasta, com mtime diferente: beta (mais nova)
aparece primeiro. O clique no alfinete da linha de alfa precisa movê-la para
o topo, gravar `pinned_sessions` no state.json e manter a ordem após reabrir.
Desafixar devolve a ordem por atividade.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-pin-") as temporary:
    temp = Path(temporary)
    project = temp / "proj"
    project.mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    now = time.time()
    paths = {}
    for name, age in (("alfa", 3600), ("beta", 0)):
        path = sessions / f"{name}.jsonl"
        path.write_text(json.dumps({"type": "session", "cwd": str(project), "id": name}) + "\n"
                        + json.dumps({"type": "session_info", "name": name}) + "\n")
        os.utime(path, (now - age, now - age))
        paths[name] = path
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    state = config / "state.json"
    state.write_text(json.dumps({"appearance": "dark"}))
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(agent),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(temp / "events.jsonl"), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")

    def launch():
        return subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(project)],
            env=env, stdout=log, stderr=log)

    def pinned():
        try:
            return [Path(path) for path in json.loads(state.read_text()).get("pinned_sessions", [])]
        except (FileNotFoundError, json.JSONDecodeError):
            return []

    def row(label):
        found = find_word(window, label)
        assert found is not None, f"linha {label} não encontrada"
        return found

    # O alfinete fica antes do botão de apagar, à direita do cartão. O clique
    # acerta o botão mesmo com opacidade 0 (o hover só o revela).
    def click_pin(label):
        current = row(label)
        click(window, 207, current[1] + current[3] // 2)
        time.sleep(0.4)

    def leave_rail(window):
        """Tira o mouse do rail: com hover, a lista congela a ordem anterior."""
        root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        x11.XTranslateCoordinates(display, window, root_window, 0, 0,
                                  ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
        xtst.XTestFakeMotionEvent(display, -1, root_x.value + 620, root_y.value + 420, 0)
        x11.XFlush(display)
        time.sleep(0.4)

    def events():
        log_path = temp / "events.jsonl"
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines()]

    def get_messages_count():
        return sum(1 for entry in events() if entry.get("command", {}).get("type") == "get_messages")

    def wait_ready(after=0):
        wait_for(lambda: find_window(), "Window not created")
        wait_for(lambda: get_messages_count() > after, "Fake session not initialized", timeout=30)
        active = find_window()
        x11.XSetInputFocus(display, active, 1, 0)
        expose_window(active)
        wait_for(lambda: find_word(active, "Pergunte"), "Composer did not render", timeout=30)
        wait_for(lambda: find_word(active, "alfa") and find_word(active, "beta"),
                 "Saved sessions did not render", timeout=20)
        return active

    process = launch()
    try:
        window = wait_ready()
        assert row("beta")[1] < row("alfa")[1], "Atividade deveria ordenar beta antes de alfa"
        click_pin("alfa")
        leave_rail(window)
        wait_for(lambda: paths["alfa"] in pinned(), "Pino não foi salvo")
        wait_for(lambda: row("alfa")[1] < row("beta")[1], "Alfa fixada não subiu ao topo")

        # Reinicia: a ordem fixada tem de sobreviver.
        before = get_messages_count()
        process.terminate()
        process.wait(timeout=5)
        log.flush()
        process = launch()
        window = wait_ready(before)
        wait_for(lambda: row("alfa")[1] < row("beta")[1], "Ordem fixada não sobreviveu ao restart")
        assert paths["alfa"] in pinned(), "Pino sumiu do state.json no restart"

        # Desafixa e a ordem por atividade volta.
        click_pin("alfa")
        leave_rail(window)
        wait_for(lambda: not pinned(), "Pino não foi removido")
        wait_for(lambda: row("beta")[1] < row("alfa")[1], "Ordem por atividade não voltou")
        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Pinned sessions smoke passed")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()
