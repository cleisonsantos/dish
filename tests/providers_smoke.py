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
        """Centro da palavra que ancora o rótulo, mesmo que o OCR a cole.

        O OCR do CI tanto divide um rótulo em palavras ("Voltar à lista" vira
        duas linhas, com o "à" lido como "&") quanto cola as palavras de um
        botão ("Confirmar remoção" vira "Confirmarremogéo"). A âncora aceita os
        dois lados: a palavra pode ser prefixo do rótulo ou começar por ele.
        Comparar sem acentos e sem espaços resolve as duas coisas, como os
        outros smokes já fazem (`click_word` em appearance_smoke).
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
                if len(word) >= 4 and (wanted.startswith(word) or word.startswith(wanted)):
                    return (int(fields[6]) + int(fields[8]) // 2, int(fields[7]) + int(fields[9]) // 2)
            time.sleep(0.4)
        return None

    def click(self, label):
        position = self.find_label(label)
        assert position, f"{self.name}: não achei {label!r} na tela:\n{self.capture()}"
        click(self.window, position[0], position[1])

    def ocr_large(self):
        """OCR do mesmo frame em 2x para rótulos pequenos do catálogo.

        O resumo do catálogo ("4 provedores") e a origem da credencial ficam em
        texto micro; no runner sem GPU o tesseract às vezes perde um caractere
        no tamanho nativo. A ampliação não muda a asserção, só a leitura."""
        large = self.screenshot + ".large.png"
        subprocess.run(["convert", self.screenshot, "-resize", "200%", large],
                       check=True, stderr=subprocess.DEVNULL)
        return subprocess.check_output(["tesseract", large, "stdout", "--psm", "6"],
                                       stderr=subprocess.DEVNULL).decode()

    def wait_text(self, needles, timeout=75):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            text = self.capture()
            if all(normalized(needle) in normalized(text) for needle in needles):
                return text
            large = self.ocr_large()
            if all(normalized(needle) in normalized(large) for needle in needles):
                return text + "\n" + large
            time.sleep(0.5)
        raise AssertionError(f"{self.name}: faltou algo em {needles!r}:\n{self.capture()}")

    def assert_absent(self, needle):
        wanted = normalized(needle)
        for _ in range(6):
            if wanted not in normalized(self.capture()):
                return
            time.sleep(0.4)
        raise AssertionError(f"{self.name}: {needle!r} continuou na tela")

    def close_settings(self):
        """Fecha as Configurações e espera o foco voltar para a conversa.

        Escape funciona na maioria dos casos, mas depois do login o foco pode
        ficar no editor de resposta que saiu da tela. O botão Fechar não depende
        de foco e é o fallback determinístico.
        """
        press("Escape")
        for _ in range(10):
            if "configura" not in self.capture().lower():
                time.sleep(0.2)
                return
            position = self.find_label("Fechar")
            if position:
                click(self.window, position[0], position[1])
            time.sleep(0.4)
        raise AssertionError(f"{self.name}: Configurações não fecharam")


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


def check_provider_refreshes_open_conversation():
    """Conectar um provedor atualiza o seletor da conversa já aberta.

    É o primeiro uso completo da #69: o usuário conecta a credencial sem sair
    do Dish, volta para a conversa e escolhe o modelo novo sem reiniciar nada.
    O rascunho digitado antes de abrir as Configurações também precisa sobreviver.
    """
    with Harness("refresh", {"DISH_PI_PACKAGE": FIXTURE, "FAKE_PI_EMPTY": "1"}) as h:
        h.wait_text(["sem credencial"])
        # Deixa um rascunho na conversa e volta para as Configurações.
        h.close_settings()
        chord("Control_L", "l")
        type_text("rascunho-preservado")
        wait_for(lambda: find_word(h.window, "rascunho"), "Rascunho não apareceu")
        press("F1")
        h.wait_text(["Provedores"])
        h.click("Provedores")
        h.wait_text(["sem credencial"])
        # Conecta a anthropic, como no caso de login.
        h.click("Conectar")
        h.wait_text(["Como entrar em anthropic", "Anthropic API key"])
        h.click("Usar")
        h.wait_text(["Cole a chave de API da Anthropic"])
        type_text("sk-ant-chave-de-teste")
        time.sleep(0.3)
        h.click("Enviar")
        h.wait_text(["Provedor anthropic conectado"])
        h.click("Voltar à lista")
        h.wait_text(["credencial salva", "1 conectados"])
        # As Configurações não podem apagar o rascunho, e o catálogo da conversa
        # precisa ser relido para incluir o modelo do provedor recém-conectado.
        h.close_settings()
        wait_for(lambda: find_word(h.window, "rascunho"), "Rascunho perdido após conectar")
        chord("Control_L", "Shift_L", "m")
        # "anthropic" fica no rodapé em texto micro e o OCR do CI às vezes não
        # o enxerga; o modelo e o set_model já provam o provedor.
        h.wait_text(["Selecionar modelo", "claude"])
        h.click("claude-fake")
        wait_for(lambda: any(
            item.get("command", {}).get("type") == "set_model"
            and item["command"].get("provider") == "anthropic"
            and item["command"].get("modelId") == "claude-fake"
            for item in h.events()), "Modelo recém-conectado não foi selecionado")
        print("providers smoke ok: modelos atualizados na conversa aberta", flush=True)


check_catalog()
check_first_use()
check_unavailable()
check_login_and_logout()
check_provider_refreshes_open_conversation()
print("Providers smoke passed: catálogo, primeiro uso, falha honesta, login, remoção e atualização da conversa")
