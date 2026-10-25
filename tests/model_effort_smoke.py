"""Xvfb: model/effort transactions with asynchronous synthetic Pi responses."""
from pathlib import Path
import unicodedata

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-effort-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    (temp / "home").mkdir()
    config = temp / "config/dish"
    config.mkdir(parents=True)
    (config / "state.json").write_text(json.dumps({"generated_titles": False, "appearance": "dark"}))
    wrapper = temp / "pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/effort_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), XDG_SESSION_TYPE="x11", DISH_EFFORT_TEST_DIR=str(temp))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    process = None
    log = open(temp / "dish.log", "w")

    def events():
        result = []
        path = temp / "events.jsonl"
        for line in path.read_text().splitlines() if path.exists() else []:
            try:
                result.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return result

    def commands(kind):
        return [item for item in events() if item.get("command", {}).get("type") == kind]

    def normalized(raw):
        return " ".join("".join(c for c in unicodedata.normalize("NFD", raw.lower()) if not unicodedata.combining(c)).split())

    def text():
        screenshot = temp / "screen.png"
        subprocess.run(["import", "-window", str(window), str(screenshot)], check=True)
        # PSM 11 loses the tail of small labels on the CI font ("sem racio");
        # PSM 6 reads the same composer row correctly. Combine both so a
        # rendering that only one segmentation can read still counts.
        raw = b"\n".join(
            subprocess.check_output(["tesseract", str(screenshot), "stdout", "--psm", psm],
                                    stderr=subprocess.DEVNULL)
            for psm in ("11", "6"))
        return normalized(raw.decode())

    def slash(model, provider=True):
        chord("Control_L", "l")
        type_text(f"/model {'synthetic ' if provider else ''}{model}")
        press("Return")
        wait_for(lambda: find_word(window, "Pergunte"), "Slash command not dispatched")

    def pick(model):
        chord("Control_L", "Shift_L", "m")
        wait_for(lambda: find_word(window, "Selecionar"), "Model picker not opened")
        type_text(model)
        press("Return")

    def assert_beta_menu():
        wait_for(lambda: find_word(window, "muito", last=True), "Beta effort not loaded")
        box = find_word(window, "muito", last=True)
        click(window, box[0] + box[2] // 2, box[1] + box[3] // 2)
        def complete_menu():
            first = find_word(window, "desligado")
            last = find_word(window, "ximo")
            if not first or not last:
                return False
            left, top = max(0, first[0] - 12), max(0, first[1] - 8)
            width = max(first[2], last[2]) + 100
            height = last[1] + last[3] - top + 8
            screenshot = temp / "menu.png"
            subprocess.run(["import", "-window", str(window), "-crop", f"{width}x{height}+{left}+{top}", str(screenshot)], check=True)
            subprocess.run(["convert", str(screenshot), "-resize", "300%", str(screenshot)], check=True)
            rendered = normalized(subprocess.check_output(["tesseract", str(screenshot), "stdout", "--psm", "6"], stderr=subprocess.DEVNULL).decode())
            return all(label in rendered for label in ["desligado", "minimo", "baixo", "medio", "alto", "muito alto", "maximo"])
        wait_for(complete_menu, "Beta ladder is incomplete")
        press("Escape")

    try:
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(project)],
                                   env=env, stdout=log, stderr=log)
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Initial Pi did not initialize")
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        wait_for(lambda: find_word(window, "baixo", last=True), "Initial alpha effort not ready")

        (temp / "hold-switch").touch()
        pick("beta")
        wait_for(lambda: any(item.get("waiting") == "hold-switch" for item in events()), "Switch did not start")
        wait_for(lambda: "atualizando" in text(), "Old effort list remained active while switching")
        (temp / "hold-switch").unlink()
        assert_beta_menu()
        assert not any(item.get("switching") and item.get("command", {}).get("type") in ["get_state", "get_available_thinking_levels"] for item in events()), "Queried capabilities before switch confirmation"

        slash("gamma", provider=False)
        wait_for(lambda: "sem raciocinio" in text(), "Non-reasoning model not handled", timeout=30)
        slash("alpha")
        wait_for(lambda: find_word(window, "baixo", last=True), "Slash switch did not refresh alpha")

        # Hold an alpha ladder, queue gamma then beta, then replay stale alpha.
        (temp / "hold-levels").touch()
        (temp / "duplicate-levels").touch()
        (temp / "hold-duplicate").touch()
        before = len(commands("get_available_thinking_levels"))
        slash("alpha")
        wait_for(lambda: len(commands("get_available_thinking_levels")) > before, "Delayed ladder not queried")
        slash("gamma")
        wait_for(lambda: "atualizando" in text(), "Pending effort not shown")
        slash("beta", provider=False)
        (temp / "hold-levels").unlink()
        assert_beta_menu()
        (temp / "duplicate-levels").unlink()
        (temp / "hold-duplicate").unlink()
        wait_for(lambda: any(item.get("duplicate_sent") == "alpha" for item in events()), "Stale response not replayed")
        assert_beta_menu()

        # A transient toast may expire or wrap differently under CI fonts.
        # Hold the recovery query so the error UI is stable until observed.
        recovery = temp / "hold-recovery"
        recovery.touch()
        slash("failure")
        wait_for(lambda: any(item.get("waiting") == "hold-recovery" for item in events()),
                 "Rejected switch did not trigger a recovery query")
        wait_for(lambda: "indisponivel" in text(), "Switch failure not shown")
        assert not find_word(window, "failure", last=True), "Failed target shown as confirmed model"
        recovery.unlink()
        assert_beta_menu()  # Failure re-reads Pi instead of guessing the active model.
        slash("beta")
        assert_beta_menu()
        (temp / "fail-levels").touch()
        slash("alpha")
        wait_for(lambda: "indisponivel" in text(), "Ladder failure not shown")
        (temp / "fail-levels").unlink()
        slash("beta")
        assert_beta_menu()
        assert not any("invalid_effort" in item for item in events()), "Reconciled against obsolete levels"
        print("Model effort smoke passed")
    except Exception:
        print(json.dumps(events(), indent=2))
        print("Rendered text:", text())
        print("Application log:", (temp / "dish.log").read_text())
        if os.environ.get("DISH_TEST_FAILURE_SCREENSHOT"):
            Path(os.environ["DISH_TEST_FAILURE_SCREENSHOT"]).write_bytes((temp / "screen.png").read_bytes())
        raise
    finally:
        if process:
            process.terminate()
            process.wait(timeout=10)
        log.close()
