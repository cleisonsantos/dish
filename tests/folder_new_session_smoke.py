"""Xvfb: o + no cabeçalho do grupo abre uma sessão nova naquela pasta.

alfa é a pasta ativa (o app inicia nela, sem `--session`); beta tem só uma
sessão salva e por isso aparece como grupo com uma linha. O + do cabeçalho de
beta não é o "Nova sessão" global, que usaria a pasta ativa: o log do fake Pi
prova o `cwd` novo. No Xvfb não há portal de diálogo nativo, então o caminho
antigo (`Escolher pasta`) ficaria esperando um seletor que nunca abre e nenhum
processo nasceria; o `get_messages` novo com o cwd de beta é, portanto, a prova
de que o clique passou pelo + do grupo.

O setup segue o de `navigation_order_smoke.py`: HOME e XDG temporários,
sessões em `PI_CODING_AGENT_DIR` e um `state.json` mínimo, deixando o padrão do
app valer. O grupo recolhido não emite linhas no probe `nav-list`, então o
teste expande o cabeçalho antes de contar ou clicar — e todas as mensagens de
falha carregam o probe inteiro.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-folder-new-") as temporary:
    temp = Path(temporary)
    (temp / "home").mkdir()
    alfa, beta = temp / "alfa", temp / "beta"
    sessions = temp / "agent" / "sessions" / "test"
    sessions.mkdir(parents=True)
    # Os títulos não repetem o nome da pasta: o OCR do cabeçalho não pode
    # confundir a linha do grupo com a da sessão salva.
    titles = {alfa: "SESSAO UM", beta: "SESSAO DOIS"}
    now = time.time()
    for project, age in ((alfa, 7200), (beta, 60)):
        project.mkdir()
        path = sessions / f"{project.name}.jsonl"
        path.write_text(json.dumps({"type": "session", "cwd": str(project), "id": project.name}) + "\n"
                        + json.dumps({"type": "session_info", "name": titles[project]}) + "\n")
        os.utime(path, (now - age, now - age))

    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    state = config / "state.json"
    # Estado mínimo do navigation_order_smoke.py: o padrão do app vale.
    state.write_text(json.dumps({"appearance": "dark"}))
    log_path = temp / "events.jsonl"
    trace_path = temp / "ui.jsonl"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(log_path),
               DISH_TEST_UI_LOG=str(trace_path), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    # A sessão ativa nasce em alfa; beta só tem a sessão salva, então qualquer
    # processo novo em beta só pode vir do + do grupo.
    process = subprocess.Popen(
        [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(alfa)],
        env=env, stdout=log, stderr=log)

    def events():
        if not log_path.exists():
            return []
        entries = []
        for line in log_path.read_text().splitlines():
            try:
                entries.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        return entries

    def commands(kind):
        return [entry for entry in events() if entry.get("command", {}).get("type") == kind]

    def started_folders():
        return [entry.get("cwd", "") for entry in commands("get_messages")]

    window = None

    def nav():
        node = UI_TRACES[window].node("nav-list") if window in UI_TRACES else None
        return node["data"] if node else {}

    def nav_titles():
        return list(nav().get("titles", []))

    def saved_rows_visible():
        current = nav_titles()
        return all(title in current for title in titles.values())

    def expand_group():
        """Grupo recolhido não emite linha no probe: reabre antes de contar."""
        if saved_rows_visible():
            return
        header = wait_for(lambda: find_word(window, beta.name),
                          f"Cabeçalho do grupo {beta.name} não apareceu\n{nav()}")
        node = wait_ui(window, "nav-list")
        click(window, round(node["bounds"][0]) + 20, header[1] + header[3] // 2)
        wait_for(saved_rows_visible,
                 f"As linhas salvas não apareceram depois de expandir o grupo\n{nav()}", timeout=20)

    def click_group_new(folder):
        """Clica no + do cabeçalho do grupo e espera um processo novo do Pi.

        O botão não tem probe; o contador ao lado propaga o clique e recolheria
        o grupo. O ponto vem da borda direita da lista (o + é o último controle
        antes do contador) e o efeito é conferido no log do Pi, nunca em pixels.
        Se o clique acertar o contador, o grupo recolhe; reabre e tenta a
        posição vizinha.
        """
        header = wait_for(lambda: find_word(window, folder),
                          f"Cabeçalho do grupo {folder} não apareceu\n{nav()}")
        y = header[1] + header[3] // 2
        node = wait_ui(window, "nav-list")
        right = round(node["bounds"][0] + node["bounds"][2])
        before = len(started_folders())

        def spawned():
            return len(started_folders()) > before

        for offset in (38, 36, 40, 34, 42):
            expand_group()
            click(window, right - offset, y)
            try:
                wait_for(spawned, "O + do grupo não iniciou uma sessão", timeout=8)
                return
            except AssertionError:
                continue
        raise AssertionError(
            f"O + do grupo {folder} não iniciou sessão perto de x={right - 38} "
            f"(borda direita da lista em x={right})\n{nav()}")

    try:
        wait_for(lambda: find_window(), "Janela do Dish não foi criada")
        window = find_window()
        UI_TRACES[window] = UiTrace(trace_path)
        prepare_window(window)
        wait_ui(window, "nav-list", "A navegação não foi pintada")
        wait_for(lambda: started_folders(), "Fake Pi não iniciou a sessão ativa", timeout=30)
        expand_group()
        # Linha da sessão salva de beta (a pasta que não é a ativa).
        wait_for(lambda: titles[beta] in nav_titles(),
                 f"A sessão salva de {beta.name} não apareceu na navegação\n{nav()}", timeout=30)

        before_sessions = len(started_folders())
        before_rows = len(nav_titles())
        before_blank = nav_titles().count("Nova conversa")
        click_group_new(beta.name)
        started = started_folders()[before_sessions:]
        assert started, f"O clique no + não subiu nenhum processo do Pi\n{nav()}"
        assert all(Path(folder).resolve() == beta.resolve() for folder in started), \
            f"Pi iniciou em {started}, não na pasta {beta}\n{nav()}"
        wait_for(lambda: len(nav_titles()) > before_rows,
                 f"A sessão nova não entrou no grupo da pasta\n{nav()}", timeout=20)
        assert nav_titles().count("Nova conversa") > before_blank, nav()
        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Folder new session smoke passed")
    except Exception:
        log.flush()
        print((temp / "dish.log").read_text())
        raise
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()
        for pid in {entry.get("pid") for entry in events() if entry.get("pid")}:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass
