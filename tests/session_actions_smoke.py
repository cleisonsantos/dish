"""Synthetic Xvfb regression: stable session actions, confirmation and exit barrier.

Run after building: xvfb-run -a python3 tests/session_actions_smoke.py.
Only temporary sessions/processes. No network or real Pi state.
"""
from pathlib import Path
import signal

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

NAV_WIDTH = 280

with tempfile.TemporaryDirectory(prefix="dish-session-actions-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    home = temp / "home"
    home.mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "synthetic"
    sessions.mkdir(parents=True)
    stop_dir = temp / "stops"
    stop_dir.mkdir()
    gate_dir = temp / "gates"
    gate_dir.mkdir()
    paths = {}
    for index, name in enumerate(("alfa", "beta", "gama")):
        path = sessions / f"{name}.jsonl"
        path.write_text(json.dumps({"type": "session", "cwd": str(project), "id": name}) + "\n"
                        + json.dumps({"type": "session_info", "name": name}) + "\n")
        os.utime(path, (time.time() - 3600 + index, time.time() - 3600 + index))
        paths[name] = path
    wrapper = temp / "fake-pi"
    wrapper.write_text("#!/usr/bin/python3\n"
                       + "FIXTURE = " + repr(str(ROOT / "tests/fixtures/fake_pi.py")) + "\n"
                       + "STOP_DIR = " + repr(str(stop_dir)) + "\n" + r'''
import json, os, signal, sys, time
from pathlib import Path

def stopping(_signum, _frame):
    current = globals().get("session_file")
    present = Path(current).is_file() if current else None
    Path(STOP_DIR, f"started-{os.getpid()}").write_text(json.dumps({"session": current, "original_present": present}))
    # A final session write after SIGTERM catches close-then-immediate-trash.
    time.sleep(1.2 if Path(STOP_DIR, f"slow-{os.getpid()}").exists() else 0.4)
    if current:
        with open(current, "a") as target:
            target.write(json.dumps({"type": "synthetic_shutdown", "pid": os.getpid()}) + "\n")
    Path(STOP_DIR, f"finished-{os.getpid()}").write_text("finished")
    raise SystemExit(0)

signal.signal(signal.SIGTERM, stopping)
for started in Path(STOP_DIR).glob("started-*"):
    previous = started.name.removeprefix("started-")
    if not Path(STOP_DIR, f"finished-{previous}").exists():
        Path(STOP_DIR, f"overlap-{os.getpid()}").write_text(previous)
source = Path(FIXTURE).read_text().replace("deadline = time.monotonic() + 30", "deadline = time.monotonic() + 120")
exec(compile(source, FIXTURE, "exec"), globals())
''')
    wrapper.chmod(0o700)
    config = temp / "config" / "dish"
    config.mkdir(parents=True)
    preferences = config / "state.json"
    preferences.write_text(json.dumps({"appearance": "dark", "navigation_open": True,
                                       "navigation_width": NAV_WIDTH, "details_open": False,
                                       "generated_titles": False, "reduce_motion": True}))
    event_path = temp / "events.jsonl"
    env = dict(os.environ, HOME=str(home), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(event_path),
               DISH_FAKE_PI_RUN_GATE_DIR=str(gate_dir), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(project)],
                               env=env, stdout=log, stderr=log)

    def events():
        if not event_path.exists():
            return []
        records = []
        for line in event_path.read_text().splitlines():
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError:
                pass  # A writer may be between write and newline.
        return records

    def commands(kind=None):
        return [entry for entry in events() if "command" in entry
                and (kind is None or entry["command"].get("type") == kind)]

    def session_pid(name):
        for entry in reversed(events()):
            if entry.get("record", {}).get("data", {}).get("sessionFile") == str(paths[name]):
                return entry["pid"]
        return None

    def alive(pid):
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def words():
        with tempfile.NamedTemporaryFile(suffix=".png") as image:
            subprocess.run(["import", "-window", str(window), image.name], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            rows = subprocess.check_output(["tesseract", image.name, "stdout", "--psm", "11", "tsv"],
                                           stderr=subprocess.DEVNULL).decode().splitlines()[1:]
        return [(fields[11], *(int(fields[i]) for i in (6, 7, 8, 9)))
                for row in rows if len(fields := row.split("\t")) == 12 and fields[11].strip()]

    def row(label, last=False):
        matches = [(x, y, width, height) for text, x, y, width, height in words()
                   if text.casefold() == label.casefold() and x < NAV_WIDTH - 48]
        return (matches[-1] if last else matches[0]) if matches else None

    def nav_words():
        """OCR the narrow rail at 3x: selected-row subtitles are small/low-contrast."""
        attributes = XWindowAttributes()
        x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
        with tempfile.NamedTemporaryFile(suffix=".png") as image:
            subprocess.run(["import", "-window", str(window), image.name], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            large = tempfile.mktemp(prefix="dish-nav-", suffix=".png")
            try:
                subprocess.run(["convert", image.name, "-crop",
                                f"{NAV_WIDTH}x{attributes.height}+0+0", "+repage",
                                "-resize", "300%", large], check=True,
                               stderr=subprocess.DEVNULL)
                rows = subprocess.check_output(
                    ["tesseract", large, "stdout", "--psm", "6", "tsv"],
                    stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            finally:
                if os.path.exists(large):
                    os.unlink(large)
        return [(fields[11], *(int(fields[i]) / 3 for i in (6, 7, 8, 9)))
                for row in rows if len(fields := row.split("\t")) == 12 and fields[11].strip()]

    def lifecycle_on_rail():
        labels = set()
        for text, x, *_ in nav_words():
            if x >= NAV_WIDTH - 48:
                continue
            normalized = text.casefold()
            labels.update(label for label in ("atual", "aberta", "salva")
                          if label in normalized)
        return labels

    def row_lifecycle(label):
        current = row(label)
        if current is None:
            return None
        # OCR often joins "Atual há 1 h" into one phrase; match the lifecycle
        # word inside the subtitle, not an exact whole-line equality.
        for text, x, y, _width, _height in nav_words():
            if x >= NAV_WIDTH - 48 or not current[1] < y < current[1] + 40:
                continue
            normalized = text.casefold()
            for state in ("atual", "aberta", "salva"):
                if state in normalized:
                    return state
        return None

    def zoom_words():
        """Retry OCR at 2x for dialog text dimmed by the scrim."""
        with tempfile.NamedTemporaryFile(suffix=".png") as image:
            subprocess.run(["import", "-window", str(window), image.name], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            large = tempfile.mktemp(prefix="dish-zoom-", suffix=".png")
            try:
                subprocess.run(["convert", image.name, "-resize", "200%", large], check=True,
                               stderr=subprocess.DEVNULL)
                rows = subprocess.check_output(
                    ["tesseract", large, "stdout", "--psm", "6", "tsv"],
                    stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            finally:
                if os.path.exists(large):
                    os.unlink(large)
        return [(fields[11], *(int(fields[i]) / 2 for i in (6, 7, 8, 9)))
                for row in rows if len(fields := row.split("\t")) == 12 and fields[11].strip()]

    def find_text(label, last=False):
        box = find_word(window, label, last=last)
        if box:
            return box
        matches = [(x, y, width, height) for text, x, y, width, height in zoom_words()
                   if label.casefold() in text.casefold()]
        return (matches[-1] if last else matches[0]) if matches else None

    def click_word(label, last=False):
        wait_for(lambda: find_text(label, last=last), f"Missing action: {label}", timeout=20)
        x, y, width, height = find_text(label, last=last)
        click(window, x + width // 2, y + height // 2)

    def ensure_row(label, last=False):
        for button in (5, 4):
            for _ in range(6):
                current = row(label, last)
                if current is not None:
                    return current
                attributes = XWindowAttributes()
                x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
                wheel_at(window, NAV_WIDTH // 2, attributes.height // 2, button, 3)
        raise AssertionError(f"Missing session: {label}")

    def menu_visible():
        return find_word(window, "Fixar") or find_word(window, "Desafixar")

    def open_menu(label, last=False):
        current = ensure_row(label, last)
        # Text locates the row; the textless trigger is to the right. Retry a
        # few points because streaming changes row height before the event
        # lands, and OCR's title box is not the button's box.
        for x, y in [(NAV_WIDTH - 24, current[1] + current[3] // 2),
                     (NAV_WIDTH - 24, current[1] + 20),
                     (NAV_WIDTH - 34, current[1] + current[3] // 2)]:
            click(window, x, y)
            try:
                wait_for(menu_visible, "", timeout=2)
                return
            except AssertionError:
                if menu_visible():
                    return
        raise AssertionError("Session menu did not open")

    def row_active(label):
        """Selected rows carry a saturated left rail; OCR misreads the label
        on the highlighted background. Sample the painted border instead."""
        current = row(label)
        if current is None:
            return False
        raw = subprocess.check_output(["import", "-window", str(window),
                                       "-depth", "8", "rgb:-"],
                                      stderr=subprocess.DEVNULL)
        attributes = XWindowAttributes()
        x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
        width = attributes.width
        y = min(attributes.height - 1, current[1] + max(1, current[3] // 2))
        for x in range(8, 24):
            offset = (y * width + x) * 3
            pixel = raw[offset:offset + 3]
            if len(pixel) == 3 and max(pixel) - min(pixel) >= 32:
                return True
        return False

    def open_session(label):
        current = ensure_row(label)
        click(window, current[0] + current[2] // 2, current[1] + current[3] // 2)
        wait_for(lambda: session_pid(label), "Pi did not open saved session", timeout=20)
        wait_for(lambda: row_active(label), "Session did not become current", timeout=20)

    def pinned(name):
        try:
            return str(paths[name]) in json.loads(preferences.read_text()).get("pinned_sessions", [])
        except (FileNotFoundError, json.JSONDecodeError):
            return False

    def dialog_visible():
        # Buttons are dimmed by the scrim and OCR poorly; body wording is unique.
        return (find_text("segundos") or find_text("interrompida")
                or find_text("Cancelar"))

    def begin_delete(name):
        open_menu(name)
        click_word("Excluir")
        wait_for(dialog_visible, "Delete confirmation missing", timeout=20)

    def confirm_delete():
        # The dialog starts on Cancelar; one Tab selects the destructive
        # action. Keyboard confirmation avoids OCR on scrim-dimmed buttons.
        chord("Tab")
        chord("Return")
        wait_for(lambda: dialog_visible() is None, "Delete confirmation did not close")

    def assert_cancel_has_no_effect(name, pid=None):
        before = len(commands("abort"))
        begin_delete(name)
        chord("Escape")
        wait_for(lambda: dialog_visible() is None, "Escape did not cancel deletion", timeout=20)
        assert paths[name].exists(), "Cancel moved the session file"
        assert len(commands("abort")) == before, "Cancel aborted Pi"
        if pid:
            assert alive(pid), "Cancel stopped Pi"
            assert not (stop_dir / f"started-{pid}").exists(), "Cancel signalled Pi"

    try:
        wait_for(lambda: find_window(), "Window not created", timeout=30)
        window = find_window()
        prepare_window(window)
        wait_for(lambda: row("alfa") and row("beta") and row("gama"), "Saved catalog missing", timeout=30)
        open_session("beta")
        wait_for(lambda: lifecycle_on_rail() >= {"atual", "aberta", "salva"},
                 "Lifecycle states are not distinct", timeout=20)
        beta_pid = session_pid("beta")

        # Trigger must never activate a saved row or spawn another process.
        before = len(commands("get_messages"))
        open_menu("alfa")
        assert len(commands("get_messages")) == before, "Menu activated saved session"
        chord("Escape")
        wait_for(lambda: find_word(window, "Fixar") is None, "Escape did not dismiss menu")
        chord("Return")  # Focus must return to the same row's trigger.
        wait_for(lambda: find_word(window, "Fixar"), "Trigger focus not restored")
        chord("Down")  # Saved Close disabled; skip straight to Delete.
        chord("Return")
        wait_for(dialog_visible, "Keyboard menu navigation failed")
        chord("Escape")
        assert paths["alfa"].exists()
        assert len(commands("get_messages")) == before
        assert_cancel_has_no_effect("alfa")

        open_menu("alfa")
        click(window, NAV_WIDTH + 100, 60)
        wait_for(lambda: find_word(window, "Fixar") is None, "Outside click did not dismiss menu")

        # Popup remains inside small viewport; its wheel never moves history.
        x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
        x11.XResizeWindow(display, window, 720, 520)
        x11.XFlush(display)
        expose_window(window)
        # At 520px height the rail scrolls; use a row that must be visible.
        wait_for(lambda: row("gama"), "Narrow navigation disappeared", timeout=20)
        history_before = [(text, x, y) for text, x, y, *_ in words() if x > NAV_WIDTH and text == "message"]
        open_menu("gama")
        for label in ("Fixar", "Fechar", "Excluir"):
            x, y, width, height = find_word(window, label)
            assert 8 <= x and x + width <= 712 and 8 <= y and y + height <= 512, (label, x, y)
        option = find_word(window, "Fixar")
        wheel_at(window, option[0] + 5, option[1] + 5, 5, 16)
        assert find_word(window, "Fixar"), "Menu wheel dismissed popup"
        assert [(text, x, y) for text, x, y, *_ in words() if x > NAV_WIDTH and text == "message"] == history_before
        chord("Escape")
        x11.XResizeWindow(display, window, 1240, 960)
        x11.XFlush(display)
        expose_window(window)
        wait_for(lambda: find_word(window, "Pergunte"), "Composer did not return", timeout=20)

        open_menu("beta")
        click_word("Fixar")
        wait_for(lambda: pinned("beta"), "Pin not persisted")
        click_word("Pergunte")
        type_text("long-activity-delete")
        chord("Return")
        wait_for(lambda: any(entry.get("pid") == beta_pid and entry.get("record", {}).get("type") == "agent_start"
                             for entry in events()), "Synthetic run did not start")
        assert_cancel_has_no_effect("beta", beta_pid)
        before = len(commands("abort"))
        begin_delete("beta")
        confirm_delete()
        wait_for(lambda: (stop_dir / f"finished-{beta_pid}").exists(), "Pi shutdown did not finish")
        wait_for(lambda: not paths["beta"].exists(), "Open session was not trashed", timeout=20)
        assert json.loads((stop_dir / f"started-{beta_pid}").read_text())["original_present"], "File moved before Pi shutdown"
        assert not alive(beta_pid), "Pi still owns a deleted session"
        trash = temp / "data" / "dish" / "trash"
        trashed = list(trash.glob("*.jsonl"))
        assert len(trashed) == 1
        assert '"synthetic_shutdown"' in trashed[0].read_text(), "Final shutdown write missing from trash"
        assert len(commands("abort")) == before, "Deletion used a second stdin control path"
        assert not pinned("beta")
        before = len(commands("get_messages"))
        click_word("Desfazer")
        wait_for(lambda: paths["beta"].exists() and pinned("beta"), "Undo did not restore file and pin")
        wait_for(lambda: (row_lifecycle("beta") or "").casefold() == "salva", "Undo resurrected a live session")
        assert len(commands("get_messages")) == before, "Undo restarted Pi"

        # Saved-file delete takes same confirmation / trash / undo path.
        begin_delete("alfa")
        confirm_delete()
        wait_for(lambda: not paths["alfa"].exists(), "Saved session was not trashed")
        # Deletion completion focuses the undo action; return must activate it
        # instead of sending the composer draft.
        time.sleep(0.4)
        chord("Return")
        wait_for(lambda: paths["alfa"].exists(), "Keyboard-only saved-file undo failed")
        open_session("alfa")
        alfa_pid = session_pid("alfa")

        # Ordinary Close must fence the newly Saved row while the old writer
        # runs its SIGTERM cleanup. Reopening then would race two Pi owners;
        # deletion shares this same path fence.
        (stop_dir / f"slow-{alfa_pid}").write_text("slow cleanup")
        open_menu("alfa")
        click_word("Fechar")
        wait_for(lambda: (stop_dir / f"started-{alfa_pid}").exists(), "Close did not request shutdown")
        current = ensure_row("alfa")
        click(window, current[0] + current[2] // 2, current[1] + current[3] // 2)
        wait_for(lambda: (stop_dir / f"finished-{alfa_pid}").exists(), "Ordinary Close did not finish")
        assert not list(stop_dir.glob("overlap-*")), "Saved row reopened before previous Pi exited"
        assert '"synthetic_shutdown"' in paths["alfa"].read_text(), "Close lost final session write"
        open_session("alfa")
        alfa_pid = session_pid("alfa")

        # Failure after closing a live process must preserve its source file
        # and explain partial outcome, rather than claiming successful delete.
        open_session("gama")
        gama_pid = session_pid("gama")
        backup = temp / "trash-backup"
        trash.rename(backup)
        trash.write_text("synthetic blocked destination")
        try:
            begin_delete("gama")
            confirm_delete()
            wait_for(lambda: find_text("fechada"), "Partial delete failure not shown", timeout=20)
            assert paths["gama"].exists(), "Trash failure lost original file"
            assert not alive(gama_pid), "Failed trash left closed Pi alive"
            wait_for(lambda: (row_lifecycle("gama") or "").casefold() == "salva", "Closed failed-delete session not saved")
        finally:
            trash.unlink()
            backup.rename(trash)

        # Leave persisted alfa as only live session; last-live actions explain
        # their disabled state, and neither closes Pi nor moves its file.
        open_menu("Nova", last=True)
        click_word("Fechar")
        wait_for(lambda: (row_lifecycle("alfa") or "").casefold() == "atual", "Surviving conversation not selected")
        open_menu("alfa")
        wait_for(lambda: find_text("Mantenha"), "Last-live guard not explained")
        click_word("Fechar")
        assert find_text("Mantenha"), "Disabled Close dismissed menu"
        click_word("Excluir")
        assert find_text("Mantenha"), "Disabled Delete dismissed menu"
        assert paths["alfa"].exists() and alive(alfa_pid)
        assert not (stop_dir / f"started-{alfa_pid}").exists()
        chord("Escape")
        assert process.poll() is None, "Dish exited during session actions"
        print("Session actions smoke passed")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-session-actions-failure-", suffix=".png")
        subprocess.run(["import", "-window", "root", screenshot], check=False)
        print(f"Failure screenshot: {screenshot}")
        raise
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        # Dish may be terminated by signal; explicitly clean only fixture PIDs.
        owned = {entry["pid"] for entry in events()}
        for pid in owned:
            try:
                os.killpg(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 2
        while any(alive(pid) for pid in owned) and time.monotonic() < deadline:
            time.sleep(0.05)
        for pid in owned:
            try:
                os.killpg(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        log.close()
