"""Xvfb UI test: abrir uma conversa em outra pasta pela lista de recentes.

A pasta vem da preferência (`recent_projects`); o clique precisa subir um
processo do Pi com o `cwd` novo e fazer `last_project` acompanhar. O diálogo
nativo não é exercitado aqui (não há portal no Xvfb) — o caminho de validação
de pasta é o mesmo do `open_project`.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-project-") as temporary:
    temp = Path(temporary)
    alpha = temp / "alpha"
    beta = temp / "beta"
    alpha.mkdir()
    beta.mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    state = config / "state.json"
    # beta já foi usada antes; alpha é a pasta da sessão aberta agora.
    state.write_text(json.dumps({"appearance": "dark", "recent_projects": [str(beta)]}))
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    process = subprocess.Popen(
        [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(alpha)],
        env=env, stdout=log, stderr=log)

    def events():
        return [json.loads(line) for line in log_path.read_text().splitlines()] if log_path.exists() else []

    def commands(kind):
        return [entry for entry in events() if entry.get("command", {}).get("type") == kind]

    def started_folders():
        return [entry.get("cwd", "") for entry in commands("get_messages")]

    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Fake session not initialized")
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
        # A sessão aberta é alpha; beta só pode aparecer em Projetos recentes.
        wait_for(lambda: find_word(window, "beta"), "Recent project row missing")
        recent = find_word(window, "beta")
        click(window, recent[0] + recent[2] // 2, recent[1] + recent[3] // 2)
        wait_for(lambda: any(folder.endswith("/beta") for folder in started_folders()),
                 "No Pi process started in beta")
        wait_for(lambda: str(json.loads(state.read_text()).get("last_project", "")).endswith("/beta"),
                 "last_project did not follow the folder")
        # A pasta nova vira um grupo na navegação; a antiga continua aberta.
        wait_for(lambda: find_word(window, "beta"), "Beta did not appear in the navigation")
        assert process.poll() is None, (temp / "dish.log").read_text()

        # Caminho inexistente na linha de comando: avisa e NÃO vira prompt.
        process.terminate()
        process.wait(timeout=5)
        log.flush()
        previous = len(commands("prompt"))
        before = len(commands("get_messages"))
        missing = temp / "nao-existe"
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
             str(missing), "algum pedido"],
            env=env, stdout=log, stderr=log)
        wait_for(lambda: len(commands("get_messages")) > before,
                 "Second launch did not initialize", timeout=30)
        time.sleep(1.5)
        window = find_window()
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        assert len(commands("prompt")) == previous, "Invalid path was sent as a prompt"
        wait_for(lambda: find_word(window, "existe"), "Missing folder notice", timeout=10)
        print("Project folder smoke passed")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()
