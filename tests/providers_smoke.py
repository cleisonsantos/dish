"""Xvfb UI test da seção Provedores: catálogo, estado, primeiro uso e login.

Os casos comuns usam só Pi sintético e o SDK de fixture: não tocam na credencial
real, não abrem rede e não dependem de um Pi instalado — `DISH_PI_PACKAGE`
aponta para `tests/fixtures/fake-pi-sdk`, que responde um catálogo conhecido e
guarda credenciais em memória.

O último caso usa o `pi` real do PATH, mas sempre com `PI_CODING_AGENT_DIR`
isolado e chaves falsas. Ele reproduz o snapshot de modelos calculado no
startup: conectar um provedor depois disso não muda `get_available_models`, e a
única correção é reabrir a conversa. Se o `pi` real não estiver instalado, esse
caso é anunciado como ignorado; os casos sintéticos continuam obrigatórios.
"""
import re
import select
import shutil
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

    def __init__(self, name, env, npm_global=False, real_pi=False):
        self.name = name
        self.real_pi = real_pi
        self.directory = tempfile.TemporaryDirectory(prefix=f"dish-providers-{name}-")
        temp = Path(self.directory.name)
        if real_pi:
            program = shutil.which("pi")
            assert program, "o caso real exige `pi` no PATH"
            wrapper = Path(program)
            # Fixture isolada: nada disto é o ~/.pi/agent de ninguém.
            agent = temp / "agent"
            agent.mkdir()
            (agent / "auth.json").write_text(json.dumps({
                "deepseek": {"type": "api_key", "key": "sk-dish-provider-smoke-fake"},
            }))
            session = agent / "sessions" / "providers-real" / f"{name}.jsonl"
            session.parent.mkdir(parents=True)
            now = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
            stamp = int(time.time() * 1000)
            session.write_text("\n".join(json.dumps(entry) for entry in [
                {"type": "session", "version": 3, "id": f"providers-real-{name}",
                 "timestamp": now, "cwd": str(temp)},
                {"type": "message", "id": "real-user-1", "parentId": None,
                 "timestamp": now, "message": {"role": "user", "timestamp": stamp,
                 "content": [{"type": "text", "text": "pergunta anterior"}]}},
                {"type": "message", "id": "real-assistant-1", "parentId": "real-user-1",
                 "timestamp": now, "message": {"role": "assistant", "timestamp": stamp + 1,
                 "provider": "deepseek", "model": "deepseek-v4-pro", "stopReason": "stop",
                 "content": [{"type": "text", "text": "resposta anterior"}],
                 "usage": {"input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0,
                           "totalTokens": 2,
                           "cost": {"input": 0, "output": 0, "cacheRead": 0,
                                    "cacheWrite": 0, "total": 0}}}},
            ]) + "\n")
        elif npm_global:
            # Instalação npm global fora dos dois layouts fixos: o pacote fica
            # em <prefixo>/lib/node_modules e bin/pi é um link para dentro dele.
            prefix = temp / "npm-global"
            package = prefix / "lib/node_modules/@earendil-works/pi-coding-agent"
            shutil.copytree(ROOT / "tests/fixtures/fake-pi-sdk", package)
            packaged_pi = package / "bin/pi"
            packaged_pi.parent.mkdir(parents=True)
            packaged_pi.write_text(
                f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
            packaged_pi.chmod(0o700)
            wrapper = prefix / "bin/pi"
            wrapper.parent.mkdir(parents=True)
            wrapper.symlink_to(packaged_pi)
            (temp / "home").mkdir()
        else:
            wrapper = temp / "fake-pi"
            wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
            wrapper.chmod(0o700)
        self.log_path = temp / "events.jsonl"
        self.trace_path = temp / "ui.jsonl"
        if not real_pi:
            session = temp / "session.jsonl"
            session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": f"providers-{name}"}) + "\n")
        merged = dict(os.environ, DISH_PI_BIN=str(wrapper),
                      PI_CODING_AGENT_DIR=str(temp / "agent"),
                      XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
                      XDG_CACHE_HOME=str(temp / "cache"),
                      DISH_FAKE_PI_LOG=str(self.log_path), DISH_TEST_UI_LOG=str(self.trace_path),
                      XDG_SESSION_TYPE="x11")
        merged.update(env)
        if real_pi:
            # Chave falsa + diretório isolado; sem tráfego de rede do Pi.
            merged["PI_OFFLINE"] = "1"
            merged.pop("DISH_PI_PACKAGE", None)
        if npm_global:
            merged["HOME"] = str(temp / "home")
            merged.pop("DISH_PI_PACKAGE", None)
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
        wait_for(lambda: find_window(), "Window not created", timeout=30)
        self.window = find_window()
        UI_TRACES[self.window] = UiTrace(self.trace_path)
        if self.real_pi:
            # O primeiro frame do Xvfb pode não ter sido exposto ainda; sem um
            # pedido de expose o OCR lê uma imagem vazia até a primeira tecla.
            prepare_window(self.window)
            self.wait_text(["pergunta anterior", "resposta anterior"],
                           timeout=30)
            wait_for(lambda: find_word(self.window, "deepseek"),
                     f"{self.name}: catálogo inicial real não carregou", timeout=30)
        else:
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
        UI_TRACES.pop(self.window, None)
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

    def click_row_action(self, provider, label):
        """Clica no botão da linha do provedor, não no primeiro botão da tela.

        A lista real tem 42 provedores; o primeiro "Conectar" visível é
        amazon-bedrock, não anthropic. O OCR localiza a linha pelo id e o botão
        pela mesma altura.
        """
        provider_wanted = normalized(provider)
        label_wanted = normalized(label)
        for _ in range(12):
            subprocess.run(["import", "-window", str(self.window), self.screenshot], check=True)
            rows = subprocess.check_output(["tesseract", self.screenshot, "stdout", "--psm", "6", "tsv"],
                                           stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            words = []
            for row in rows:
                fields = row.split("\t")
                if len(fields) != 12:
                    continue
                text = normalized(fields[11])
                if not text:
                    continue
                x, y = int(fields[6]), int(fields[7])
                width, height = int(fields[8]), int(fields[9])
                words.append((text, x, y, width, height))
            anchors = [word for word in words
                       if word[0].startswith(provider_wanted) or provider_wanted.startswith(word[0])]
            actions = [word for word in words
                       if len(word[0]) >= 4
                       and (word[0].startswith(label_wanted) or label_wanted.startswith(word[0]))]
            for anchor in anchors:
                anchor_y = anchor[2] + anchor[4] / 2
                same_row = [action for action in actions
                            if abs((action[2] + action[4] / 2) - anchor_y) <= max(12, anchor[4])]
                if same_row:
                    action = min(same_row, key=lambda word: word[1])
                    click(self.window, action[1] + action[3] // 2, action[2] + action[4] // 2)
                    return
            time.sleep(0.4)
        raise AssertionError(
            f"{self.name}: não achei {label!r} na linha de {provider!r}:\n{self.capture()}")

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

    def assert_masked_answer(self, key):
        """Confirma o estado pintado e que a chave não aparece na tela ou no trace."""
        type_text(key)
        node = wait_ui(self.window, "providers-answer", condition=lambda node:
                       node["data"].get("length") == len(key))
        assert node["data"].get("masked") is True, f"{self.name}: resposta não está mascarada"
        readback = normalized(self.capture())
        assert normalized(key) not in readback, f"{self.name}: valor real apareceu na tela"
        assert normalized("nao-deve-aparecer") not in readback, (
            f"{self.name}: parte do valor real apareceu na tela")
        assert key not in self.trace_path.read_text(), f"{self.name}: valor real apareceu no trace"

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
        h.wait_text(["pacote", "Informe o executável do Pi", "Verificar de novo",
                     "Abrir o Pi num terminal"])
        # O OCR às vezes lê o acento do botão como outra vogal; a âncora curta
        # não depende disso.
        assert h.find_label("Escolher"), "sem botão para escolher o executável"


def check_npm_global():
    """Catálogo carrega com o Pi fora dos layouts fixos (npm global)."""
    with Harness("npm-global", {}, npm_global=True) as h:
        h.wait_text(["openai", "credencial salva", "anthropic", "4 provedores"])
        print("providers smoke ok: Pi de npm global fora dos layouts fixos", flush=True)


def check_login_and_logout():
    """Conectar por chave de API e remover com confirmação."""
    with Harness("login", {"DISH_PI_PACKAGE": FIXTURE, "FAKE_PI_EMPTY": "1"}) as h:
        h.wait_text(["sem credencial"])
        # anthropic é a primeira linha sem credencial.
        h.click("Conectar")
        h.wait_text(["Como entrar em anthropic", "Anthropic API key"])
        h.click("Usar")
        h.wait_text(["Cole a chave de API da Anthropic"])
        key = "sk-test-nao-deve-aparecer-123"
        # O valor existe no buffer (vai no envio) mas não pode ser desenhado.
        h.assert_masked_answer(key)
        h.click("Enviar")
        # O valor sai da tela ao ser enviado: nada dele fica renderizado.
        h.assert_absent(key)
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


def assert_real_pi_model_not_found(program):
    """Mede o defeito direto no RPC do Pi real, antes de testar a UI.

    Agente isolado, chaves falsas, `PI_OFFLINE=1`: nenhum valor real e nenhuma
    rede. A credencial da anthropic é gravada *depois* do startup; a lista não
    muda e `set_model` responde `Model not found`, exatamente como na seção 7.3.
    """
    with tempfile.TemporaryDirectory(prefix="dish-real-pi-stale-") as directory:
        temp = Path(directory)
        agent = temp / "agent"
        agent.mkdir()
        home = temp / "home"
        home.mkdir()
        (agent / "auth.json").write_text(json.dumps({
            "deepseek": {"type": "api_key", "key": "sk-dish-provider-smoke-fake"},
        }))
        env = dict(os.environ, HOME=str(home), PI_CODING_AGENT_DIR=str(agent),
                   PI_OFFLINE="1")
        env.pop("DISH_PI_PACKAGE", None)
        env.pop("DISH_PI_BIN", None)
        process = subprocess.Popen(
            [program, "--mode", "rpc"], cwd=temp, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, bufsize=1)

        next_id = [0]

        def request(kind, **fields):
            next_id[0] += 1
            request_id = next_id[0]
            process.stdin.write(json.dumps({"id": request_id, "type": kind, **fields}) + "\n")
            process.stdin.flush()
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                ready, _, _ = select.select([process.stdout], [], [], 0.5)
                if not ready:
                    continue
                line = process.stdout.readline()
                if not line:
                    break
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if record.get("type") == "response" and record.get("id") == request_id:
                    return record
            raise AssertionError(f"Pi real não respondeu a {kind}")

        try:
            first = request("get_available_models")
            providers = sorted({model["provider"] for model in first["data"]["models"]})
            assert providers == ["deepseek"], providers
            auth = json.loads((agent / "auth.json").read_text())
            auth["anthropic"] = {"type": "api_key", "key": "sk-ant-dish-provider-smoke-fake"}
            (agent / "auth.json").write_text(json.dumps(auth))
            stale = request("get_available_models")
            stale_providers = sorted({model["provider"] for model in stale["data"]["models"]})
            assert stale_providers == ["deepseek"], stale_providers
            selection = request("set_model", provider="anthropic", modelId="claude-haiku-4-5")
            assert selection.get("success") is False, selection
            assert selection.get("error") == "Model not found: anthropic/claude-haiku-4-5", selection
        finally:
            try:
                process.stdin.close()
            except OSError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()


def check_real_pi_stale_catalog_and_reopen():
    """Pi real: o snapshot de startup não vê uma credencial conectada depois.

    Diretório de agente isolado, chaves falsas e `PI_OFFLINE=1`: nenhuma
    credencial real é lida, escrita ou enviada a um serviço. O teste mede o
    `Model not found` no RPC real e depois verifica, na UI, o aviso honesto e a
    ação que reinicia o processo mantendo rascunho e histórico.
    """
    program = shutil.which("pi")
    if not program:
        print("providers smoke: caso com Pi real ignorado — `pi` não está no PATH", flush=True)
        return
    assert_real_pi_model_not_found(program)
    with Harness("real", {}, real_pi=True) as h:
        # O __enter__ já abriu as Configurações; aqui a conversa precisa estar
        # visível para o rascunho e a checagem de histórico.
        h.close_settings()
        h.wait_text(["pergunta anterior", "resposta anterior"])
        chord("Control_L", "l")
        type_text("rascunho-real-preservado")
        wait_for(lambda: find_word(h.window, "rascunho"), "Rascunho real não apareceu")
        press("F1")
        h.wait_text(["Provedores"])
        h.click("Provedores")
        h.wait_text(["deepseek", "credencial salva", "anthropic"])
        # anthropic é a segunda linha; o primeiro "Conectar" seria amazon-bedrock.
        h.click_row_action("anthropic", "Conectar")
        h.wait_text(["Como entrar em anthropic", "Anthropic API key"])
        h.click("Usar")
        # O Pi real usa a mensagem do provedor ("Enter Anthropic API key"), não
        # o texto em português do SDK de fixture.
        h.wait_text(["Enter Anthropic API key", "Enviar"])
        type_text("sk-ant-chave-real-de-teste")
        time.sleep(0.3)
        h.click("Enviar")
        h.wait_text(["Provedor anthropic conectado"])
        h.click("Voltar à lista")
        # O resumo ("2 conectados") fica no rodapé de 42 linhas e sai da tela;
        # a credencial salva da anthropic é a prova visível.
        h.wait_text(["anthropic", "credencial salva", "Remover credencial"])
        h.close_settings()
        # O processo continua com o snapshot antigo: a conversa não pode dizer
        # que atualizou; ela avisa que falta anthropic e oferece reabrir.
        h.wait_text(["desatualizado", "anthropic", "Reabrir esta conversa"], timeout=30)
        wait_for(lambda: find_word(h.window, "rascunho"), "Rascunho perdido antes de reabrir")
        h.click("Reabrir esta conversa")
        # Histórico vem do mesmo --session; o rascunho nunca saiu do editor.
        h.wait_text(["pergunta anterior", "resposta anterior"], timeout=30)
        h.assert_absent("desatualizado")
        wait_for(lambda: find_word(h.window, "rascunho"), "Rascunho perdido ao reabrir")
        # O processo novo calculou o snapshot com as duas credenciais.
        chord("Control_L", "Shift_L", "m")
        h.wait_text(["Selecionar modelo"], timeout=30)
        type_text("claude-haiku-4-5")
        h.wait_text(["claude-haiku-4-5"], timeout=30)
        press("Escape")
        print("providers smoke ok: Pi real avisa o snapshot velho e reabre sem perder rascunho/histórico", flush=True)


check_catalog()
check_first_use()
check_unavailable()
check_npm_global()
check_login_and_logout()
check_provider_refreshes_open_conversation()
check_real_pi_stale_catalog_and_reopen()
print("Providers smoke passed: catálogo, npm global, primeiro uso, falha honesta, login, remoção, atualização da conversa e snapshot velho do Pi real")
