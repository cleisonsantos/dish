"""Run with xvfb-run -a python3 tests/startup_smoke.py after cargo build --release.

Uses only fake Node/npm/Pi executables. No network or user configuration changes.
"""
from pathlib import Path

# Reuse XTest helpers without running the existing-session test scenario.
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split(
    'with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-startup-") as temporary:
    temp = Path(temporary)
    bin_dir = temp / "bin"
    bin_dir.mkdir()
    log_path = temp / "events.jsonl"
    install_log = temp / "install-called"
    terminal_log = temp / "terminal-called"
    node_log = temp / "node-called"

    def script(name, content):
        path = bin_dir / name
        path.write_text("#!/bin/sh\n" + content + "\n")
        path.chmod(0o700)
        return path

    # O verificador do caminho npm roda `node --version` em segundo plano; o
    # fixture registra cada chamada para o teste esperar a verificação sem OCR.
    script("node", f'printf "v22.19.0"; printf "node\\n" >> "{node_log}"')
    script("x-terminal-emulator", f'''printf '%s\\n' "$@" > "{terminal_log}"''')
    script("npm", f'''if [ "$1" = --version ]; then printf '10.0.0'; exit 0; fi
test "$1" = install && test "$3" = --ignore-scripts || exit 2
printf 'installed\\n' >> "{install_log}"
/bin/mkdir -p "$7/bin"
printf '#!/bin/sh\\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\\n' > "$7/bin/pi"
/bin/chmod +x "$7/bin/pi"
''')
    env = dict(os.environ, PATH=str(bin_dir),
               XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"),
               PI_CODING_AGENT_DIR=str(temp / "agent"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11",
               RUST_BACKTRACE="1")
    env.pop("DISH_PI_BIN", None)
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)

    def events():
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines() if line]

    def launch():
        log = open(temp / "dish.log", "w")
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(temp)], env=env,
            stdout=log, stderr=log)
        return process, log

    def click_text_once(label):
        # One OCR pass: True when the button was found and clicked.
        # Fonts and line wrapping vary between local desktops and clean CI:
        # find the button's text instead of assuming its vertical position.
        # OCR may drop spaces and read a lone "o" as zero ("Abrir 0 Dish").
        def normalized(value):
            return "".join(value.lower().split()).replace("0", "o")

        # The primary button's single line can be skipped by page
        # segmentation at native size; 200% keeps it readable. Coordinates
        # below are scaled back down before the click.
        subprocess.run(
            ["import", "-window", str(window), "-resize", "200%", screenshot],
            check=True)
        rows = subprocess.check_output(
            ["tesseract", screenshot, "stdout", "--psm", "6", "tsv"],
            stderr=subprocess.DEVNULL).decode().splitlines()[1:]
        lines = {}
        for row in rows:
            fields = row.split("\t")
            if len(fields) == 12 and fields[11].strip():
                lines.setdefault(tuple(fields[1:5]), []).append(fields)
        needle = normalized(label)
        best = None
        for words in lines.values():
            # Two actions can share one OCR line. Match the contiguous
            # words for this label instead of clicking the line's middle.
            ordered = sorted(words, key=lambda word: int(word[6]))
            for start in range(len(ordered)):
                for end in range(start + 1, len(ordered) + 1):
                    subset = ordered[start:end]
                    text = normalized("".join(word[11] for word in subset))
                    if needle in text and (best is None or len(text) < best[0]):
                        best = (len(text), subset)
        if best is None:
            return False
        subset = best[1]
        left = min(int(word[6]) for word in subset) // 2
        right = max(int(word[6]) + int(word[8]) for word in subset) // 2
        first = subset[0]
        click(window, (left + right) // 2,
              (int(first[7]) + int(first[9]) // 2) // 2)
        return True

    def click_text(label):
        wait_for(lambda: click_text_once(label), f"Button not found: {label}")

    # O assistente também segue a aparência; o visual verificado é o escuro.
    (temp / "config/dish").mkdir(parents=True, exist_ok=True)
    (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))
    process, log = launch()
    try:
        wait_for(lambda: find_window(), "Setup window not created")
        window = find_window()
        # O Xvfb não tem gerenciador de janelas: sem o expose que um desktop
        # real envia ao mapear a janela, o primeiro quadro do assistente pode
        # nunca ser pintado (captura preta, OCR sem nada para ler).
        prepare_window(window)
        time.sleep(1)
        assert process.poll() is None
        assert not install_log.exists(), "Installation ran without consent"
        assert not terminal_log.exists(), "Official installer ran without consent"
        screenshot = tempfile.mktemp(prefix="dish-setup-", suffix=".png")

        def screen_text():
            subprocess.run(
                ["import", "-window", str(window), "-resize", "200%", screenshot],
                check=True)
            return subprocess.check_output(
                ["tesseract", screenshot, "stdout", "--psm", "6"],
                stderr=subprocess.DEVNULL).decode()

        def screen_has(label):
            # OCR may drop spaces or read a lone "o" as zero, as in click_text.
            def normalized(value):
                return "".join(value.lower().split()).replace("0", "o")

            return normalized(label) in normalized(screen_text())

        # Primeiro passo: boas-vindas, sem instalar nada. Esc fecha limpo.
        assert screen_has("Bem-vindo"), screen_text()
        press("Escape")
        wait_for(lambda: process.poll() is not None, "Escape did not close setup cleanly")
        process.wait(timeout=5)
        log.close()
        process, log = launch()
        wait_for(lambda: find_window(), "Setup window not recreated after escape")
        window = find_window()
        prepare_window(window)
        time.sleep(1)

        # Segundo passo: verificação do Pi e escolha do caminho de instalação.
        press("Tab")
        press("Return")
        wait_for(lambda: screen_has("Verificar o Pi"), "Pi verification did not render")
        click_text("Instalar o Pi")
        wait_for(lambda: screen_has("Instalar via npm"),
                 "Install step did not render")
        install_page = screen_text()
        assert "install.sh" in install_page, install_page
        # Voltar percorre o fluxo linear sem perder a verificação encontrada.
        click_text("Voltar")
        wait_for(lambda: screen_has("Verificar o Pi"), "Voltar did not return to Pi check")
        click_text("Instalar o Pi")
        wait_for(lambda: screen_has("Instalar via npm"),
                 "Install step did not return after Voltar")
        print(f"Setup screenshot: {screenshot}", flush=True)
        # Official consent launches only a fake terminal, never a remote script.
        click_text("Aceitar e abrir instalador oficial")
        wait_for(lambda: terminal_log.exists(), "Official consent did not open terminal")
        arguments = terminal_log.read_text().splitlines()
        assert arguments[:3] == ["-e", "/bin/sh", "-c"]
        assert "curl -fsSL https://pi.dev/install.sh | sh" in arguments[3]
        assert not install_log.exists(), "Official option invoked npm instead"
        process.terminate()
        process.wait(timeout=5)
        log.close()
        process, log = launch()
        wait_for(lambda: find_window(), "Setup window not recreated")
        window = find_window()
        prepare_window(window)
        time.sleep(2)
        # Segundo passo do assistente: a alternativa sem terminal. O foco
        # inicial fica no botão primário; Tab + Return aciona "Começar" sem
        # depender de OCR do rótulo, como no fluxo oficial acima.
        press("Tab")
        press("Return")
        wait_for(lambda: screen_has("Verificar o Pi"), "Pi verification did not render again")
        click_text("Instalar o Pi")
        wait_for(lambda: screen_has("Instalar via npm"), "npm path was not offered")
        click_text("Instalar via npm")
        wait_for(lambda: screen_has("Verificar o Node"), "Node step did not render")
        assert not install_log.exists(), "npm path installed before Node consent"
        # O botão de consentimento (`setup-install`, startup.rs) só é desenhado
        # quando `node_ready()` é verdadeiro, mas o rótulo "Instalar via npm" é
        # idêntico ao do passo Install por OCR — esperar pela string pode
        # passar antes da troca de tela. A verificação roda `node --version`
        # duas vezes (installation::prerequisites e node_version), e a segunda
        # só acontece depois de Node 22.19+ e npm passarem; logo em seguida
        # `check_node` grava a versão e a tela ganha o botão. Nenhum outro passo
        # chama Node, então duas chamadas no log não podem vir do passo anterior.
        def node_calls():
            if not node_log.exists():
                return 0
            return len(node_log.read_text().splitlines())

        wait_for(lambda: node_calls() >= 2, "Node check did not finish")
        click_text("Instalar via npm")
        wait_for(lambda: install_log.exists(), "Consent did not start fake installation")
        wait_for(lambda: screen_has("Pronto"), "Installation did not reach the end state")
        # Voltar depois do fim e continuar volta ao mesmo estado pronto.
        click_text("Voltar")
        wait_for(lambda: screen_has("Verificar o Pi"), "Voltar did not leave the end state")
        click_text("Continuar")
        wait_for(lambda: screen_has("Pronto"), "Pi found did not return to the end state")
        # A instalação leva ao passo Pronto; abrir o Dish confirma a escolha.
        # O rótulo do botão primário do passo Done depende de o workspace já
        # existir atrás do assistente: "Abrir o Dish" no primeiro uso e
        # "Voltar ao Dish" quando a conexão em segundo plano já criou o
        # workspace (startup.rs: open_label = if reopened). Os dois fazem a
        # mesma coisa — fechar o assistente e entrar no workspace — então o
        # teste clica no que o estado real ditar.
        def click_done():
            for label in ("Abrir o Dish", "Voltar ao Dish"):
                if click_text_once(label):
                    print(f"Done button label used: {label}", flush=True)
                    return True
            return False
        wait_for(click_done, "Done button not found")
        wait_for(lambda: any(e.get("command", {}).get("type") == "get_messages" for e in events()),
                 "Installed Pi did not initialize")
        preferences = temp / "config/dish/state.json"
        wait_for(lambda: preferences.exists(), "Selected executable was not saved")
        assert json.loads(preferences.read_text())["pi_executable"] == str(temp / "data/dish/pi/bin/pi")
        initial_count = sum(e.get("command", {}).get("type") == "get_messages" for e in events())
        process.terminate()
        process.wait(timeout=5)
        log.close()
        process, log = launch()
        wait_for(lambda: sum(e.get("command", {}).get("type") == "get_messages" for e in events()) > initial_count,
                 "Installed Pi was not discovered after restart")
        assert install_log.read_text().splitlines() == ["installed"], "Restart reinstalled Pi"
        wait_for(lambda: find_window(), "Workspace window not recreated")
        window = find_window()
        prepare_window(window)
        time.sleep(1)
        # O assistente pode ser reaberto em Configurações → Sobre / Pi; Esc
        # volta ao workspace já aberto em vez de encerrar o Dish.
        press("F1")
        wait_for(lambda: screen_has("Configura"), "Settings did not open")
        click_text("Sobre")
        wait_for(lambda: screen_has("Reabrir o assistente"), "About page did not render")
        click_text("Reabrir o assistente")
        wait_for(lambda: screen_has("Bem-vindo ao Dish"), "Wizard did not reopen")
        press("Escape")
        wait_for(lambda: not screen_has("Bem-vindo ao Dish"), "Escape did not close the wizard")
        assert process.poll() is None, "Cancelling the wizard closed the workspace"
        print("Startup installation smoke passed")
    except Exception:
        print((temp / "dish.log").read_text())
        # A captura por janela pode sair preta sem WM; a raiz mostra o X inteiro.
        root_shot = tempfile.mktemp(prefix="dish-setup-root-", suffix=".png")
        subprocess.run(["import", "-window", "root", root_shot], check=False)
        print(f"Root screenshot: {root_shot}", flush=True)
        raise
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        log.close()
        for pid in {e["pid"] for e in events()}:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass
