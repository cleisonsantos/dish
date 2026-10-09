"""Xvfb: activity/attention/read state, keyboard filters and XDG persistence.
Only synthetic Pi, temporary projects and session files; no network.
"""
from pathlib import Path
import unicodedata

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


def normalized(text):
    return " ".join("".join(c for c in unicodedata.normalize("NFD", text.lower())
                            if not unicodedata.combining(c)).split())


with tempfile.TemporaryDirectory(prefix="dish-activity-") as temporary:
    temp = Path(temporary)
    (temp / "home").mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    projects = [temp / "alpha", temp / "beta"]
    for project in projects:
        project.mkdir()
        (sessions / f"{project.name}.jsonl").write_text(
            json.dumps({"type": "session", "cwd": str(project), "id": project.name}) + "\n" +
            json.dumps({"type": "session_info", "name": f"{project.name} saved"}) + "\n")
    original_sessions = {path: path.read_bytes() for path in sessions.glob("*.jsonl")}
    gates = temp / "run-gates"
    gates.mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    preferences_path = temp / "config/dish/state.json"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(log_path), DISH_FAKE_PI_RUN_GATE_DIR=str(gates), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    screenshot = temp / "navigation.png"
    log = open(temp / "dish.log", "w")
    process = None
    window = None

    def events():
        if not log_path.exists():
            return []
        # The fake's writer may still be appending the last line.
        records = []
        for line in log_path.read_text().splitlines():
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return records

    def commands(kind):
        return [event for event in events() if event.get("command", {}).get("type") == kind]

    def preferences():
        try:
            return json.loads(preferences_path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def error_mentions():
        """Quantas vezes o erro sintético aparece na janela (banner + transcript)."""
        capture = temp / "window.png"
        subprocess.run(["import", "-window", str(window), str(capture)], check=True)
        rows = subprocess.check_output(["tesseract", str(capture), "stdout", "--psm", "6", "tsv"],
                                       stderr=subprocess.DEVNULL).decode().splitlines()[1:]
        return len([row for row in rows if len(row.split("\t")) == 12 and "synthet" in normalized(row.split("\t")[11])])

    def banner_visible():
        """O banner tinge de rosa a faixa logo acima do composer; a nota do
        transcript tem só uma barra fina e não altera a média da faixa."""
        capture = temp / "window.png"
        subprocess.run(["import", "-window", str(window), str(capture)], check=True)
        height = int(subprocess.check_output(["identify", "-format", "%h", str(capture)]).decode())
        mean = float(subprocess.check_output([
            "convert", str(capture), "-crop", f"520x22+320+{height - 162}",
            "-format", "%[fx:mean.r-mean.b]", "info:"]).decode())
        return mean > -0.015

    def nav_text():
        subprocess.run(["import", "-window", str(window), "-crop", "264x860+0+0", str(screenshot)], check=True)
        return normalized(subprocess.check_output(["tesseract", str(screenshot), "stdout", "--psm", "6"],
                                                  stderr=subprocess.DEVNULL).decode())

    def start():
        global process, window
        before = len(commands("get_messages"))
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
                                    "--session", str(sessions / "alpha.jsonl"), str(projects[0])],
                                   env=env, stdout=log, stderr=log)
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: len(commands("get_messages")) > before, "Initial session did not initialize")
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        chord("Control_L", "l")
        wait_for(lambda: "todas" in nav_text(), "Initial navigation unavailable")

    def select_title(title):
        chord("Control_L", "k")
        chord("Control_L", "a")
        press("BackSpace")
        type_text(title)
        wait_for(lambda: title in nav_text(), f"Missing session: {title}")
        press("Down")
        press("Return")
        project_name = title.split()[0]
        wait_for(lambda: Path(preferences().get("last_project", "")).name == project_name,
                 f"Selected session did not become active: {title}")
        chord("Control_L", "k")
        chord("Control_L", "a")
        press("BackSpace")
        wait_for(lambda: "filtrar" in nav_text(), "Search did not clear after opening a session")
        chord("Control_L", "l")

    def send(text):
        before = len(commands("prompt"))
        chord("Control_L", "l")
        type_text(text)
        press("Return")
        wait_for(lambda: len(commands("prompt")) > before, f"Prompt not sent: {text}")
        return commands("prompt")[-1]["pid"]

    def release_run(message):
        (gates / message).touch()

    def cycle_filter():
        chord("Control_L", "Alt_L", "f")

    def completed(pid, count=1):
        return len([event for event in events() if event["pid"] == pid and
                    event.get("record", {}).get("type") == "agent_end"]) >= count

    try:
        start()
        assert len(commands("get_messages")) == 1, "Catalog started extra processes"
        cycle_filter()  # Precisa de você
        wait_for(lambda: "nenhuma" in nav_text(), "Attention filter included idle/saved sessions")
        cycle_filter()  # Não lidas
        wait_for(lambda: "nenhuma" in nav_text(), "History was incorrectly marked unread")
        cycle_filter()  # Executando
        wait_for(lambda: "nenhuma" in nav_text(), "Running filter included open idle sessions")
        cycle_filter()  # Todas
        alpha_pid = send("long-activity-a")
        select_title("beta saved")
        wait_for(lambda: len(commands("get_messages")) == 2, "Keyboard did not open saved session")
        release_run("long-activity-a")
        wait_for(lambda: completed(alpha_pid), "Background alpha did not finish")
        alpha_path = str(sessions / "alpha.jsonl")
        beta_path = str(sessions / "beta.jsonl")
        wait_for(lambda: alpha_path in preferences().get("unread_sessions", {}), "Background response not persisted")
        wait_for(lambda: "resposta nova" in nav_text(), "Unread response not shown")

        # Two queued beta runs: the first response remains unread while the next runs.
        beta_pid = send("long-activity-b")
        send("long-activity-queued")
        wait_for(lambda: "em fila" in nav_text(), "Queued prompt has no separate indicator")
        chord("Control_L", "Tab")  # alpha
        wait_for(lambda: alpha_path not in preferences().get("unread_sessions", {}), "Viewing alpha did not mark it read")
        release_run("long-activity-b")
        wait_for(lambda: completed(beta_pid), "First beta run did not finish")
        wait_for(lambda: beta_path in preferences().get("unread_sessions", {}), "Beta response not persisted")
        wait_for(lambda: "resposta nova" in nav_text() and "executando" in nav_text(), "Unread and executing did not coexist")
        release_run("long-activity-queued")
        wait_for(lambda: completed(beta_pid, 2), "Queued beta run did not finish")
        assert not commands("abort"), "Navigation interrupted an execution"

        select_title("beta saved")
        send("dialog")
        chord("Control_L", "Tab")  # alpha, leaving beta's dialog pending
        wait_for(lambda: any(event.get("record", {}).get("type") == "extension_ui_request" for event in events()), "No blocking dialog")
        wait_for(lambda: "precisa de voce" in nav_text(), "Pending dialog not shown")
        cycle_filter()  # Precisa de você
        wait_for(lambda: "beta saved" in nav_text() and "alpha saved" not in nav_text(), "Attention filter is incorrect")
        press("Left")
        wait_for(lambda: "beta saved" not in nav_text(), "Keyboard did not fold the highlighted project")
        press("Right")
        wait_for(lambda: "beta saved" in nav_text(), "Keyboard did not restore the folded project")
        press("Down")
        press("Return")
        type_text("answer")
        press("Return")
        wait_for(lambda: commands("extension_ui_response"), "Keyboard could not answer the pending dialog")
        cycle_filter(); cycle_filter(); cycle_filter()  # back to Todas
        chord("Control_L", "l")
        wait_for(lambda: completed(beta_pid, 3), "Dialog run did not finish")
        send("test-interrupted")
        wait_for(lambda: completed(beta_pid, 4), "Interrupted test did not finish")
        wait_for(lambda: "interrompida" in nav_text(), "Explicit interruption not displayed")
        assert " erro " not in f" {nav_text()} ", "Interruption was incorrectly classified as error"
        send("test-error")
        wait_for(lambda: completed(beta_pid, 5), "Error test did not finish")
        wait_for(lambda: " erro " in f" {nav_text()} ", "Explicit error not displayed")
        # O erro só veio em turn_end: o banner aparece; a nota é verificada
        # logo após a dispensa, quando qualquer menção restante é a nota.
        wait_for(banner_visible, "Turn-end error banner missing")

        send("test-empty")
        chord("Control_L", "Tab")  # alpha
        wait_for(lambda: completed(beta_pid, 6), "Empty run did not finish")
        assert beta_path not in preferences().get("unread_sessions", {}), "Bare agent_end created an unread response"
        select_title("beta saved")
        # A nova execução limpou o banner antigo.
        wait_for(lambda: not banner_visible(), "New run did not clear the stale error banner")
        send("test-error")
        wait_for(lambda: completed(beta_pid, 7), "Second error test did not finish")
        wait_for(banner_visible, "Second error banner missing")
        press("Escape")
        wait_for(lambda: not banner_visible(), "Escape did not dismiss the banner")
        # Com o banner dispensado, a menção restante é a nota do transcript.
        wait_for(lambda: error_mentions() >= 1, "Escape removed the error record")
        send("long-activity-restart")
        chord("Control_L", "Tab")  # alpha
        release_run("long-activity-restart")
        wait_for(lambda: completed(beta_pid, 8), "Restart test did not finish")
        wait_for(lambda: beta_path in preferences().get("unread_sessions", {}), "Unread state missing before restart")
        # Fold beta's project, then navigation: both must retain aggregate indicators.
        click_nav_label(window, "beta")
        wait_for(lambda: "nova" in nav_text(), "Folded project lacks unread aggregate")
        chord("Control_L", "Shift_L", "b")
        wait_for(lambda: "1n" in nav_text().replace(" ", ""), "Collapsed rail lacks aggregate count")
        chord("Control_L", "Shift_L", "b")
        process.terminate()
        process.wait(timeout=5)
        wait_for(lambda: not find_window(), "Old window did not close")
        # Make restart layout deterministic and also verify minimum-size preferences.
        prefs = preferences()
        prefs["navigation_open"] = True
        prefs["collapsed_projects"] = []
        prefs["session_filter"] = "Unread"
        prefs["window_size"] = [720, 520]
        preferences_path.write_text(json.dumps(prefs))
        before = len(commands("get_messages"))
        start()
        wait_for(lambda: "beta saved" in nav_text() and "alpha saved" not in nav_text(), "Unread saved response lost on restart")
        assert len(commands("get_messages")) == before + 1, "Unread history started a process automatically"
        assert beta_path in preferences().get("unread_sessions", {})
        select_title("beta saved")
        wait_for(lambda: beta_path not in preferences().get("unread_sessions", {}), "Reopening saved unread response did not mark it read")
        cycle_filter(); cycle_filter()  # Unread -> Running -> All
        send("test-exit")
        wait_for(lambda: " erro " in f" {nav_text()} ", "Transport closure not displayed as error")
        cycle_filter(); cycle_filter(); cycle_filter()  # Running
        wait_for(lambda: "nenhuma" in nav_text() or "nada corresponde" in nav_text(), "Exited process is still shown as executing")
        assert not (temp / "home/.config/dish/state.json").exists(), "Ignored XDG_CONFIG_HOME"
        for path, original in original_sessions.items():
            assert path.read_bytes() == original, "Dish modified a Pi session file"
        assert process.poll() is None, (temp / "dish.log").read_text()
        print("Session activity smoke passed: filters, queue, dialog, errors, interruption, banner lifecycle, transport, persistence and keyboard")
    except Exception:
        failure = tempfile.mktemp(prefix="dish-activity-failure-", suffix=".png")
        subprocess.run(["import", "-window", str(window) if window else "root", failure], check=False)
        print(f"Activity failure screenshot: {failure}")
        if window: print(f"Navigation OCR: {nav_text()}")
        print(f"Preferences: {preferences()}")
        print(f"Event boundaries: {[event for event in events() if event.get('command', {}).get('type') in ('prompt', 'get_messages') or event.get('record', {}).get('type') in ('agent_start', 'agent_end')]}")
        print((temp / "dish.log").read_text())
        raise
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        log.close()
        for pid in {event["pid"] for event in events()}:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass
