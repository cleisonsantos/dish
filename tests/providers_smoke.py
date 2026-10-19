"""Xvfb UI test da seção Provedores: catálogo, estado e primeiro uso.

Só Pi sintético e o SDK de fixture. Não usa a credencial real, não abre rede e
não depende de um Pi instalado — `DISH_PI_PACKAGE` aponta para
`tests/fixtures/fake-pi-sdk`, que responde um catálogo conhecido.
"""
from pathlib import Path
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


def run_case(name, extra_env, expected):
    """Abre Configurações → Provedores e confere o que a tela mostra."""
    with tempfile.TemporaryDirectory(prefix=f"dish-providers-{name}-") as temporary:
        temp = Path(temporary)
        wrapper = temp / "fake-pi"
        wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
        wrapper.chmod(0o700)
        log_path = temp / "events.jsonl"
        session = temp / "session.jsonl"
        session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": f"providers-{name}"}) + "\n")
        env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
                   XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
                   XDG_CACHE_HOME=str(temp / "cache"),
                   DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11")
        env.update(extra_env)
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("PI_CODING_AGENT_SESSION_DIR", None)
        (temp / "config/dish").mkdir(parents=True, exist_ok=True)
        (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))
        log = open(temp / "dish.log", "w")
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
            env=env, stdout=log, stderr=log)
        screenshot = tempfile.mktemp(prefix=f"dish-providers-{name}-", suffix=".png")

        def capture():
            time.sleep(0.4)
            subprocess.run(["import", "-window", str(window), screenshot], check=True)
            return subprocess.check_output(["tesseract", screenshot, "stdout", "--psm", "6"],
                                           stderr=subprocess.DEVNULL).decode()

        def open_providers():
            press("F1")
            time.sleep(1.2)
            wait_for(lambda: find_word(window, "Provedores") is not None, "Configurações não abriu", timeout=15)
            entry = find_word(window, "Provedores")
            click(window, entry[0] + entry[2] // 2, entry[1] + entry[3] // 2)

        def events():
            return [json.loads(line) for line in log_path.read_text().splitlines()] if log_path.exists() else []

        try:
            wait_for(lambda: find_window(), "Window not created")
            window = find_window()
            wait_for(lambda: any(entry.get("command", {}).get("type") == "get_messages" for entry in events()),
                     f"{name}: sessão sintética não inicializou", timeout=20)
            x11.XSetInputFocus(display, window, 1, 0)
            x11.XFlush(display)
            time.sleep(1)
            open_providers()
            wait_for(lambda: any(needle in capture() for needle in expected),
                     f"{name}: nada esperado apareceu", timeout=30)
            text = capture()
            for needle in expected:
                assert needle in text, f"{name}: faltou {needle!r} em:\n{text}"
            for needle in extra_env.get("FORBIDDEN", "").split("|"):
                if needle:
                    assert needle not in text, f"{name}: não podia aparecer {needle!r}"
            print(f"providers smoke ok: {name}", flush=True)
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except Exception:
                process.kill()


FIXTURE = str(ROOT / "tests/fixtures/fake-pi-sdk")

# 1. Catálogo com um provedor conectado: os três aparecem, e só o conectado
#    anuncia origem. Nenhum segredo plantado na fixture pode chegar à tela.
run_case("catalogo", {
    "DISH_PI_PACKAGE": FIXTURE,
    "FORBIDDEN": "FIXTURE|sk-ant|eyJ",
}, ["openai", "credencial salva", "anthropic", "sem credencial", "3 provedores"])

# 2. Primeiro uso sem credencial: o convite existe em vez de uma lista vazia.
run_case("sem-credencial", {
    "DISH_PI_PACKAGE": FIXTURE,
    "FAKE_PI_EMPTY": "1",
}, ["Nenhum provedor conectado", "Conectar um provedor", "Verificar de novo"])

# 3. Sem pacote do Pi: a tela diz o motivo, não finge catálogo vazio.
run_case("sem-pacote", {
    "DISH_PI_PACKAGE": "/nonexistent/pi-package",
}, ["pacote", "Verificar de novo"])

print("Providers smoke passed: catálogo, primeiro uso e falha honesta")
