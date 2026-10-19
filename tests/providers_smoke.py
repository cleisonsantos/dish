"""Xvfb UI test da seção Provedores: catálogo, estado, primeiro uso e login.

Só Pi sintético e o SDK de fixture. Não usa a credencial real, não abre rede e
não depende de um Pi instalado — `DISH_PI_PACKAGE` aponta para
`tests/fixtures/fake-pi-sdk`, que responde um catálogo conhecido e guarda
credenciais em memória.
"""
import re
import unicodedata
from pathlib import Path
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

FIXTURE = str(ROOT / "tests/fixtures/fake-pi-sdk")


def normalized(text):
    """Compara ignorando acentos, espaços, pontuação e caixa.

    O OCR quebra linha onde quer e às vezes lê "ação" como "acao"."""
    folded = unicodedata.normalize("NFKD", text.lower())
    return re.sub(r"[^0-9a-z]+", "", folded)


class Harness:
    """Abre o Dish, a seção Provedores e oferece as ações que os casos usam."""

    def __init__(self, name, env):
        self.name = name
        self.directory = tempfile.TemporaryDirectory(prefix=f"dish-providers-{name}-")
        temp = Path(self.directory.name)
        wrapper = temp / "fake-pi"
        wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
        wrapper.chmod(0o700)
        self.log_path = temp / "events.jsonl"
        session = temp / "session.jsonl"
        session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": f"providers-{name}"}) + "\n")
        merged = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
                      XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
                      XDG_CACHE_HOME=str(temp / "cache"),
                      DISH_FAKE_PI_LOG=str(self.log_path), XDG_SESSION_TYPE="x11")
        merged.update(env)
        merged.pop("WAYLAND_DISPLAY", None)
        merged.pop("PI_CODING_AGENT_SESSION_DIR", None)
        (temp / "config/dish").mkdir(parents=True, exist_ok=True)
        (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))
        self.log = open(temp / "dish.log", "w")
        self.process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
            env=merged, stdout=self.log, stderr=self.log)
        self.screenshot = tempfile.mktemp(prefix=f"dish-providers-{name}-", suffix=".png")

    def __enter__(self):
        self.window = None
        wait_for(lambda: find_window(), "Window not created")
        self.window = find_window()
        wait_for(lambda: any(entry.get("command", {}).get("type") == "get_messages" for entry in self.events()),
                 f"{self.name}: sessão sintética não inicializou", timeout=20)
        x11.XSetInputFocus(display, self.window, 1, 0)
        x11.XFlush(display)
        time.sleep(1)
        press("F1")
        time.sleep(1.2)
        wait_for(lambda: find_word(self.window, "Provedores") is not None, "Configurações não abriu", timeout=15)
        self.click("Provedores")
        return self

    def __exit__(self, *_):
        self.process.terminate()
        try:
            self.process.wait(timeout=5)
        except Exception:
            self.process.kill()
        self.directory.cleanup()

    def events(self):
        return [json.loads(line) for line in self.log_path.read_text().splitlines()] if self.log_path.exists() else []

    def capture(self):
        time.sleep(0.4)
        subprocess.run(["import", "-window", str(self.window), self.screenshot], check=True)
        return subprocess.check_output(["tesseract", self.screenshot, "stdout", "--psm", "6"],
                                       stderr=subprocess.DEVNULL).decode()

    def find_label(self, label):
        """Centro da primeira palavra que começa o rótulo.

        A âncora é a palavra inicial, não a linha inteira: o OCR do CI já dividiu
        "Voltar à lista" em duas linhas e leu o "à" como "&". Comparar acentos e
        espaços para fora resolve as duas coisas, e é o que os outros smokes já
        fazem (`click_word` em appearance_smoke).
        """
        wanted = normalized(label)
        for _ in range(4):
            subprocess.run(["import", "-window", str(self.window), self.screenshot], check=True)
            rows = subprocess.check_output(["tesseract", self.screenshot, "stdout", "--psm", "6", "tsv"],
                                           stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            for row in rows:
                fields = row.split("\t")
                if len(fields) != 12:
                    continue
                word = normalized(fields[11])
                if len(word) >= 4 and wanted.startswith(word):
                    return (int(fields[6]) + int(fields[8]) // 2, int(fields[7]) + int(fields[9]) // 2)
            time.sleep(0.4)
        return None

    def click(self, label):
        position = self.find_label(label)
        assert position, f"{self.name}: não achei {label!r} na tela:\n{self.capture()}"
        click(self.window, position[0], position[1])

    def wait_text(self, needles, timeout=30):
        for _ in range(int(timeout / 0.5)):
            text = self.capture()
            haystack = normalized(text)
            if all(normalized(needle) in haystack for needle in needles):
                return text
            time.sleep(0.5)
        raise AssertionError(f"{self.name}: faltou algo em {needles!r}:\n{self.capture()}")

    def assert_absent(self, needle):
        wanted = normalized(needle)
        for _ in range(6):
            if wanted not in normalized(self.capture()):
                return
            time.sleep(0.4)
        raise AssertionError(f"{self.name}: {needle!r} continuou na tela")


def check_catalog():
    """Catálogo com um provedor conectado; nenhum segredo plantado aparece."""
    with Harness("catalogo", {"DISH_PI_PACKAGE": FIXTURE, "FORBIDDEN": "FIXTURE|sk-ant|eyJ"}) as h:
        text = h.wait_text(["openai", "credencial salva", "anthropic", "sem credencial", "4 provedores"])
        for planted in ("FIXTURE", "sk-ant-", "eyJ"):
            assert planted not in text, f"segredo na tela: {planted}"


def check_first_use():
    """Sem credencial: explicação e ação em vez de lista vazia."""
    with Harness("sem-credencial", {"DISH_PI_PACKAGE": FIXTURE, "FAKE_PI_EMPTY": "1"}) as h:
        h.wait_text(["Nenhum provedor conectado", "sem credencial", "Verificar de novo", "Comece pela linha"])


def check_unavailable():
    """Sem pacote do Pi: a tela diz o motivo e oferece o terminal."""
    with Harness("sem-pacote", {"DISH_PI_PACKAGE": "/nonexistent/pi-package"}) as h:
        h.wait_text(["pacote", "Verificar de novo", "Abrir o Pi num terminal"])


def check_login_and_logout():
    """Conectar por chave de API e remover com confirmação."""
    with Harness("login", {"DISH_PI_PACKAGE": FIXTURE, "FAKE_PI_EMPTY": "1"}) as h:
        h.wait_text(["sem credencial"])
        # anthropic é a primeira linha sem credencial.
        h.click("Conectar")
        h.wait_text(["Como entrar em anthropic", "Anthropic API key"])
        h.click("Usar")
        h.wait_text(["Cole a chave de API da Anthropic"])
        type_text("sk-ant-chave-de-teste")
        time.sleep(0.3)
        h.click("Enviar")
        # O valor sai da tela ao ser enviado: nada dele fica renderizado.
        h.assert_absent("chave-de-teste")
        h.wait_text(["Provedor anthropic conectado"])
        h.click("Voltar à lista")
        # A lista é relida do Pi: a credencial agora aparece como salva.
        h.wait_text(["credencial salva", "1 conectados"])
        h.click("Remover credencial")
        h.wait_text(["Confirmar"])
        h.click("Confirmar")
        h.wait_text(["Credencial de anthropic removida"])
        h.wait_text(["0 conectados"])
        print("providers smoke ok: login e remoção", flush=True)


check_catalog()
check_first_use()
check_unavailable()
check_login_and_logout()
print("Providers smoke passed: catálogo, primeiro uso, falha honesta, login e remoção")
