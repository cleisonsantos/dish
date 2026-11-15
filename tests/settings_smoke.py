"""Xvfb UI test: only synthetic Pi and isolated configuration."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import re
import threading
import unicodedata
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


class RegistryHandler(BaseHTTPRequestHandler):
    """Local npm-metadata stub: never reaches the real registry during a smoke."""

    def do_GET(self):
        body = json.dumps({
            "name": "@earendil-works/pi-coding-agent",
            "version": "0.84.5",
        }).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


registry = ThreadingHTTPServer(("127.0.0.1", 0), RegistryHandler)
threading.Thread(target=registry.serve_forever, daemon=True).start()

with tempfile.TemporaryDirectory(prefix="dish-settings-") as temporary:
    temp = Path(temporary)
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    trace = UiTrace(temp / "ui.jsonl")
    log_path = temp / "events.jsonl"
    session = temp / "session.jsonl"
    session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": "settings-test"}) + "\n")
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11",
               DISH_TEST_UI_LOG=str(trace.path),
               DISH_PI_REGISTRY_URL=f"http://127.0.0.1:{registry.server_port}/latest")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    # A aparência segue o desktop por padrão; estes testes verificam o visual
    # escuro, então a preferência fica fixada como numa instalação existente.
    (temp / "config/dish").mkdir(parents=True, exist_ok=True)
    (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))

    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
                               env=env, stdout=log, stderr=log)
    screenshot = tempfile.mktemp(prefix="dish-settings-", suffix=".png")

    def events():
        return [json.loads(line) for line in log_path.read_text().splitlines()] if log_path.exists() else []

    def commands(kind):
        return [entry for entry in events() if entry.get("command", {}).get("type") == kind]

    def probe(identity):
        return trace.node(identity)

    def capture():
        # Texto de 12px (Geist) escapa do OCR em 1x: lê a captura ampliada em
        # dois modos e devolve os dois textos juntos.
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        enlarged = screenshot.replace(".png", "-ocr.png")
        subprocess.run(["convert", screenshot, "-resize", "200%", enlarged], check=True,
                       stderr=subprocess.DEVNULL)
        return "\n".join(
            subprocess.check_output(["tesseract", enlarged, "stdout", "--psm", psm],
                                    stderr=subprocess.DEVNULL).decode()
            for psm in ("6", "11"))

    def settings_open():
        # A sonda da página vale mais que o OCR do título serifado.
        return probe("settings-page") is not None

    def click_probe(identity):
        """Clica no centro visível de um elemento pela sonda, sem coordenada fixa."""
        wait_for(lambda: probe(identity) and visible_point(probe(identity)),
                 f"Probe not painted: {identity}")
        x, y = visible_point(probe(identity))
        click(window, x, y)

    def click_label(label):
        capture()
        # Rótulos de 13px sobre o painel escuro escapam do OCR em 1x, e o modo
        # de bloco uniforme (psm 6) descarta linhas do menu que não alinham com
        # as do conteúdo. Tenta a captura ampliada e o modo esparso (psm 11)
        # antes de concluir que o rótulo não existe.
        for scale, psm in ((1, "6"), (2, "6"), (2, "11")):
            image = screenshot
            if scale != 1:
                image = screenshot.replace(".png", f"-x{scale}.png")
                subprocess.run(["convert", screenshot, "-resize", f"{scale * 100}%", image], check=True)
            rows = subprocess.check_output(["tesseract", image, "stdout", "--psm", psm, "tsv"], stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            lines = {}
            for row in rows:
                fields = row.split("\t")
                if len(fields) < 12 or not fields[11].strip():
                    continue
                lines.setdefault(tuple(fields[1:5]), []).append(fields)
            for words in lines.values():
                if label.lower() in " ".join(word[11] for word in words).lower():
                    # PSM 6 can put the heading and close button on the same
                    # OCR line. Click the label's first word, not the line's.
                    first_word = label.lower().split()[0]
                    first = next((word for word in words if first_word in word[11].lower()), words[0])
                    click(window, int(first[6]) // scale + 5, (int(first[7]) + int(first[9]) // 2) // scale)
                    return
        raise AssertionError(f"Missing UI label: {label}")

    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Fake session not initialized")
        # A lista de modelos chega em paralelo; as configurações mostram uma
        # cópia dela, então espera antes de abrir a tela.
        wait_for(lambda: commands("get_available_models"), "Model list was not requested")
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(1)
        press("F1")
        time.sleep(1)
        wait_for(settings_open, "Settings did not open")
        initial_processes = len(commands("get_messages"))
        chord("Control_L", "n")
        chord("Control_L", "Tab")
        chord("Control_L", "w")
        assert len(commands("get_messages")) == initial_processes, "Settings leaked workspace shortcuts"
        print(f"Settings screenshot: {screenshot}", flush=True)
        # Leave modal open for initial layout verification; assertions below are
        # driven by its fixed 840x620 layout at the default 1240x860 window.
        type_text("zzznomatch")
        wait_for(lambda: "Nenhum atalho" in capture(), "Shortcut filter did not show its empty state")
        press("Escape")
        # O fechamento é animado: no runner sem GPU o modal ainda aparece por
        # alguns frames, então espera-se sumir em vez de dormir um valor fixo.
        wait_for(lambda: not settings_open(), "Escape did not close settings", timeout=20)
        # As configurações abrem pelo trilho (o botão saiu do cabeçalho da gaveta).
        click_probe("rail-settings")
        time.sleep(1)
        page = capture()
        assert "Aplicativo" in page and "Contas e provedores" not in page, page
        # Modelos: ocultar/reexibir é preferência do Dish e precisa persistir.
        state_path = temp / "config/dish/state.json"
        # A troca de seção é animada e a lista de modelos chega do Pi em
        # paralelo. Repetir o clique até a página mostrar o modelo evita
        # capturar o frame antigo em vez de concluir que ele nunca chegou.
        def open_models():
            click_label("Modelos")
            page = capture()
            return "Modelos" in page and ("fake-model" in page or "test" in page)

        wait_for(open_models, "Models page did not render the Pi model", timeout=20)
        models_page = capture()
        # Switch do provedor: desliga o grupo inteiro e volta a ligar.
        click_probe("provider-switch-test")
        wait_for(lambda: json.loads(state_path.read_text()).get("hidden_models") == ["test/fake-model"],
                 "Provider switch did not hide its models")
        click_probe("provider-switch-test")
        wait_for(lambda: json.loads(state_path.read_text()).get("hidden_models") == [],
                 "Provider switch did not restore its models")
        # Switch do modelo: continua funcionando isoladamente.
        click_probe("model-switch-test/fake-model")
        wait_for(lambda: json.loads(state_path.read_text()).get("hidden_models") == ["test/fake-model"],
                 "Hidden model was not persisted")
        click_label("Mostrar")
        wait_for(lambda: json.loads(state_path.read_text()).get("hidden_models") == [],
                 "Mostrar todos did not clear the hidden models")
        # O probe confirma a seção montada; o OCR abaixo confirma sua pintura.
        def open_about():
            click_label("Sobre / Pi")
            node = probe("settings-page")
            return node is not None and node["data"]["section"] == "about" and node["data"]["about_ready"]

        wait_for(open_about, "About page did not render")
        preferences = temp / "config/dish/state.json"

        def folded_ocr():
            return "".join(
                character for character in unicodedata.normalize("NFKD", capture().lower())
                if not unicodedata.combining(character)
            )

        def about_painted():
            compact = "".join(folded_ocr().split())
            # O OCR às vezes funde os pontos da versão ("01.0"); a sonda já
            # confirmou a seção, aqui basta a versão pintada.
            return re.search(r"0\.?1\.?0", compact) is not None and "dish" in compact

        wait_for(about_painted, "About page did not paint the version and the app name")

        def pi_update_row_ready():
            compact = "".join(folded_ocr().split())
            return "verificaragora" in compact and "instalarautomaticamente" in compact

        wait_for(pi_update_row_ready, "Pi update row did not render")
        click_label("Verificar agora")
        wait_for(lambda: "dispon" in folded_ocr(),
                 "Manual Pi check did not show the registry version", timeout=20)
        wait_for(lambda: json.loads(preferences.read_text()).get("last_pi_update_check"),
                 "Manual Pi check was not persisted")
        click_label("Verificar automaticamente")
        wait_for(lambda: json.loads(preferences.read_text()).get("pi_auto_check") is True,
                 "Automatic Pi check switch was not persisted")
        click_label("Verificar automaticamente")
        wait_for(lambda: json.loads(preferences.read_text()).get("pi_auto_check") is False,
                 "Automatic Pi check switch did not turn off")

        def open_app():
            click_label("Aplicativo")
            return "Painel de detalhes" in capture()

        wait_for(open_app, "App page did not render")

        def hide_details():
            click_label("Painel de detalhes")
            return json.loads(preferences.read_text()).get("details_open") is False

        wait_for(hide_details, "Details setting not persisted")

        def hide_thinking():
            click_label("expandido")
            return json.loads(preferences.read_text()).get("show_thinking") is False

        wait_for(hide_thinking, "Thinking setting not persisted")
        # Closing button restores input focus and leaves no second help modal.
        click_label("Fechar")
        wait_for(lambda: not settings_open(), "Close button did not close")
        type_text("draft")
        press("F1")
        time.sleep(0.4)
        assert "Atalhos de teclado" in capture()
        # A página cobre a conversa; o trilho (x < 56) continua à vista e
        # clicar num ponto vazio dele volta para a conversa.
        click(window, 28, 400)
        # Mesmo close() animado do Esc: sem GPU o modal ainda aparece por alguns
        # frames, então espera-se sumir em vez de comparar uma captura única.
        wait_for(lambda: not settings_open(), "Outside click did not close", timeout=20)
        press("Return")
        wait_for(lambda: commands("prompt"), "Composer focus was not restored")
        assert commands("prompt")[-1]["command"]["message"] == "draft"
        # Preferences and shortcuts remain usable at the minimum window size.
        process.terminate()
        process.wait(timeout=5)
        log.close()
        prefs = json.loads(preferences.read_text())
        prefs["window_size"] = [720, 520]
        preferences.write_text(json.dumps(prefs))
        log = open(temp / "dish.log", "w")
        # Novo processo, novo rastro de pintura: o app cria o arquivo com
        # create_new, então o rastro anterior sai antes do restart.
        (temp / "ui.jsonl").unlink(missing_ok=True)
        trace = UiTrace(temp / "ui.jsonl")
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
                                   env=env, stdout=log, stderr=log)
        time.sleep(2)
        window = find_window()
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(0.5)
        press("F1")
        time.sleep(0.5)
        wait_for(settings_open, "Settings did not open at minimum size")
        click_label("Aplicativo")
        assert "expandido" in capture().lower(), "Preferences unavailable in small window"
        click_label("Fechar")
        wait_for(lambda: not settings_open(), "Close button did not close at minimum size")
        print("Settings basic modal smoke passed", flush=True)
    except Exception:
        print(f"Settings failure screenshot: {screenshot}")
        print((temp / "dish.log").read_text())
        raise
    finally:
        process.terminate()
        process.wait(timeout=5)
        log.close()
        registry.shutdown()
        for pid in {entry["pid"] for entry in events()}:
            try: os.kill(pid, 15)
            except ProcessLookupError: pass
