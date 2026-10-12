"""Xvfb UI test: a aparência escolhida dirige a paleta, sem OCR de cores.

Mede a luminância média da janela (pixel, não cor nomeada) para não depender
de fonte nem de posição, e confirma que a escolha persiste entre execuções.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-appearance-") as temporary:
    temp = Path(temporary)
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    session = temp / "session.jsonl"
    session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": "appearance-test"}) + "\n")
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    state = config / "state.json"
    state.write_text(json.dumps({"appearance": "dark"}))
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    screenshot = tempfile.mktemp(prefix="dish-appearance-", suffix=".png")
    log = open(temp / "dish.log", "w")

    def launch():
        return subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
             "--session", str(session), str(temp)],
            env=env, stdout=log, stderr=log)

    def events():
        return [json.loads(line) for line in log_path.read_text().splitlines()] if log_path.exists() else []

    def commands(kind):
        return [entry for entry in events() if entry.get("command", {}).get("type") == kind]

    def luminance():
        """Luminância média de 0 a 255: captura a pintura, não um pixel fixo."""
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        mean = subprocess.check_output(
            ["convert", screenshot, "-colorspace", "Gray", "-format", "%[fx:mean*255]", "info:"],
            stderr=subprocess.DEVNULL).decode()
        return float(mean)

    def ocr_lines():
        """Palavras do OCR agrupadas por linha, como nos outros testes."""
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        rows = subprocess.check_output(
            ["tesseract", screenshot, "stdout", "--psm", "6", "tsv"],
            stderr=subprocess.DEVNULL).decode().splitlines()[1:]
        lines = {}
        for row in rows:
            fields = row.split("\t")
            if len(fields) == 12 and fields[11].strip():
                lines.setdefault(tuple(fields[1:5]), []).append(fields)
        return list(lines.values())

    def click_word(label, minimum_x=0):
        """Clica na primeira palavra cujo texto começa por `label`."""
        prefix = label.casefold()
        for line in ocr_lines():
            for fields in line:
                text = fields[11].strip().casefold()
                if text.startswith(prefix) and int(fields[6]) >= minimum_x:
                    click(window, int(fields[6]) + int(fields[8]) // 2,
                          int(fields[7]) + int(fields[9]) // 2)
                    return True
        return False

    def click_segment(label):
        """Clica no cursor certo do controle segmentado (Sistema/Escuro/Claro).

        Em modo claro a OCR degrada os rótulos ("Claro" vira "claw", "Escuro"
        vira "esco"), então as âncoras são o começo das palavras, não o texto
        exato — e a linha ainda é identificada por reunir os três rótulos.
        """
        wanted = label.casefold()
        for line in ocr_lines():
            words = [fields[11].strip().casefold() for fields in line]
            anchors = all(any(word.startswith(prefix) for word in words)
                          for prefix in ("sistema", "esc", "cla"))
            if not anchors:
                continue
            for fields in line:
                text = fields[11].strip().casefold()
                if text[:3] == wanted[:3]:
                    click(window, int(fields[6]) + int(fields[8]) // 2,
                          int(fields[7]) + int(fields[9]) // 2)
                    return True
        return False

    def switch_to(label, dark):
        """Troca o tema com retry limitado: um clique perdido não reprova."""
        for _ in range(3):
            if click_segment(label):
                try:
                    wait_for(lambda: (luminance() < 90) == dark, f"tema {label}", timeout=6)
                    return
                except AssertionError:
                    continue
            time.sleep(0.5)
        raise AssertionError(f"tema {label} não aplicou")

    process = launch()
    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Fake session not initialized")
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
        time.sleep(0.6)
        assert luminance() < 90, "preferência escura não abriu o tema escuro"

        press("F1")
        time.sleep(1)
        assert click_word("apar"), "seção Aparência não encontrada"
        time.sleep(0.6)

        # A troca precisa valer sem reiniciar, nos dois sentidos.
        switch_to("claro", dark=False)
        switch_to("escuro", dark=True)

        # E precisa persistir: o evento salva o state.json na hora.
        wait_for(lambda: json.loads(state.read_text()).get("appearance") == "dark",
                 "preferência não foi salva")

        # Nova execução lê a preferência salva antes da primeira pintura.
        state.write_text(json.dumps({"appearance": "light"}))
        process.terminate()
        process.wait(timeout=5)
        log.flush()
        previous_runs = len(commands("get_messages"))
        process = launch()
        wait_for(lambda: len(commands("get_messages")) > previous_runs,
                 "Segunda execução não iniciou")
        time.sleep(0.8)
        window = find_window()
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
        assert luminance() > 160, "preferência clara não abriu o tema claro"
        print("Appearance smoke passed")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
        log.close()
