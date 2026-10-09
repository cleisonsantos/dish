"""Xvfb UI test: only synthetic Pi and isolated configuration."""
from pathlib import Path
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-settings-") as temporary:
    temp = Path(temporary)
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    session = temp / "session.jsonl"
    session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": "settings-test"}) + "\n")
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(temp / "agent"),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
                               env=env, stdout=log, stderr=log)
    screenshot = tempfile.mktemp(prefix="dish-settings-", suffix=".png")

    def events():
        return [json.loads(line) for line in log_path.read_text().splitlines()] if log_path.exists() else []

    def commands(kind):
        return [entry for entry in events() if entry.get("command", {}).get("type") == kind]

    def capture():
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        return subprocess.check_output(["tesseract", screenshot, "stdout"], stderr=subprocess.DEVNULL).decode()

    def click_label(label):
        capture()
        rows = subprocess.check_output(["tesseract", screenshot, "stdout", "tsv"], stderr=subprocess.DEVNULL).decode().splitlines()[1:]
        lines = {}
        for row in rows:
            fields = row.split("\t")
            if len(fields) < 12 or not fields[11].strip():
                continue
            lines.setdefault(tuple(fields[1:5]), []).append(fields)
        for words in lines.values():
            if label.lower() in " ".join(word[11] for word in words).lower():
                first = words[0]
                click(window, int(first[6]) + 5, int(first[7]) + int(first[9]) // 2)
                return
        raise AssertionError(f"Missing UI label: {label}")

    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Fake session not initialized")
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(1)
        press("F1")
        time.sleep(1)
        assert "Configura" in capture(), capture()
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
        time.sleep(0.4)
        assert "Configura" not in capture(), "Escape did not close settings"
        click(window, 239, 25)
        time.sleep(1)
        page = capture()
        assert "Aplicativo" in page and "Contas e provedores" not in page, page
        click_label("Sobre / Pi")
        about = capture()
        compact_about = "".join(about.lower().split())
        assert "piselecionado" in compact_about and "sdkdopi" not in compact_about, about
        click_label("Aplicativo")
        click_label("Painel de detalhes")
        click_label("expandido")
        preferences = temp / "config/dish/state.json"
        wait_for(lambda: json.loads(preferences.read_text()).get("details_open") is False, "Details setting not persisted")
        wait_for(lambda: json.loads(preferences.read_text()).get("show_thinking") is False, "Thinking setting not persisted")
        # Closing button restores input focus and leaves no second help modal.
        click_label("Fechar")
        assert "Configura" not in capture(), "Close button did not close"
        type_text("draft")
        press("F1")
        time.sleep(0.4)
        assert "Atalhos de teclado" in capture()
        click(window, 30, 400)
        assert "Configura" not in capture(), "Outside click did not close"
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
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), "--session", str(session), str(temp)],
                                   env=env, stdout=log, stderr=log)
        time.sleep(2)
        window = find_window()
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(0.5)
        press("F1")
        time.sleep(0.5)
        assert "Configura" in capture()
        click_label("Aplicativo")
        assert "expandido" in capture().lower(), "Preferences unavailable in small window"
        click_label("Fechar")
        assert "Configura" not in capture()
        print("Settings basic modal smoke passed", flush=True)
    except Exception:
        print(f"Settings failure screenshot: {screenshot}")
        print((temp / "dish.log").read_text())
        raise
    finally:
        process.terminate()
        process.wait(timeout=5)
        log.close()
        for pid in {entry["pid"] for entry in events()}:
            try: os.kill(pid, 15)
            except ProcessLookupError: pass
