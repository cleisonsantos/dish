"""Xvfb: compact navigation preserves each project's collapsed state.

Synthetic Pi and two temporary projects. Compare icon rows in the compact rail,
not arbitrary click coordinates: hiding alpha must remove its live and saved
session markers while beta's markers remain. Check reopen and restart too.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


with tempfile.TemporaryDirectory(prefix="dish-nav-collapse-") as temporary:
    temp = Path(temporary)
    (temp / "home").mkdir()
    projects = [temp / "alpha", temp / "beta"]
    sessions = temp / "agent/sessions/test"
    sessions.mkdir(parents=True)
    for project, title in zip(projects, ["ALPHACLOSED", "BETAVISIBLE"]):
        project.mkdir()
        (sessions / f"{project.name}.jsonl").write_text(
            json.dumps({"type": "session", "cwd": str(project), "id": project.name}) + "\n"
            + json.dumps({"type": "session_info", "name": title}) + "\n")
    config = temp / "config/dish"
    config.mkdir(parents=True)
    state_path = config / "state.json"
    state_path.write_text(json.dumps({
        "appearance": "dark", "theme": "dish-dark", "reduce_motion": True,
        "details_open": False, "navigation_open": True, "window_size": [1240, 760],
    }))
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    event_path = temp / "events.jsonl"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(event_path),
               XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w+")
    window = None
    process = None

    def initialized():
        count = 0
        if event_path.exists():
            for line in event_path.read_text().splitlines():
                try:
                    count += json.loads(line).get("command", {}).get("type") == "get_messages"
                except json.JSONDecodeError:
                    pass
        return count

    def preferences():
        try:
            return json.loads(state_path.read_text())
        except (OSError, json.JSONDecodeError):
            return {}

    def closed_projects():
        return set(preferences().get("collapsed_projects", []))

    def launch():
        global window
        before = initialized()
        child = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(projects[0])],
            env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: initialized() > before, "Pi did not initialize")
            wait_for(lambda: find_window(), "Window not created")
            window = find_window()
            prepare_window(window)
            wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
        except Exception:
            child.terminate()
            child.wait(timeout=5)
            raise
        return child

    def toggle_navigation(opened):
        chord("Control_L", "Shift_L", "b")
        wait_for(lambda: preferences().get("navigation_open") is opened,
                 "Navigation state was not saved")
        park_mouse(window)

    def compact_icon_rows():
        # Foreground only, inside the 44px compact rail. Surface backgrounds,
        # hairlines and selected-row borders are below this brightness cutoff.
        # Merge short internal gaps (e.g. slider icon strokes), not distinct rows.
        attributes = XWindowAttributes()
        assert x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
        raw = subprocess.check_output([
            "import", "-window", str(window), "-crop", f"44x{attributes.height}+0+0",
            "-depth", "8", "rgb:-"], stderr=subprocess.DEVNULL)
        rows = []
        for y in range(len(raw) // (44 * 3)):
            if any(max(raw[(y * 44 + x) * 3:(y * 44 + x) * 3 + 3]) >= 135
                   for x in range(10, 35)):
                rows.append(y)
        return sum(index == 0 or y - rows[index - 1] > 4 for index, y in enumerate(rows))

    try:
        process = launch()
        wait_for(lambda: find_word(window, "ALPHACLOSED") and find_word(window, "BETAVISIBLE"),
                 "Both saved projects must be visible for the positive control")
        assert not closed_projects()
        toggle_navigation(False)
        all_rows = compact_icon_rows()
        assert all_rows >= 7, "Positive control: compact rail did not expose session icons"
        toggle_navigation(True)
        wait_for(lambda: find_word(window, "ALPHACLOSED"), "Saved alpha did not return")
        click_nav_label(window, "alpha")
        expected_closed = {str(projects[0])}
        wait_for(lambda: closed_projects() == expected_closed, "Alpha collapse was not saved")
        wait_for(lambda: not find_word(window, "ALPHACLOSED"), "Alpha folder did not close")
        assert find_word(window, "BETAVISIBLE"), "Collapsing alpha hid unrelated beta"

        toggle_navigation(False)
        reduced_rows = compact_icon_rows()
        # Alpha contains one live and one saved session. Only these two rows go;
        # project icons, beta sessions, settings and branding must remain.
        assert all_rows - reduced_rows == 2, ("Compact rail ignored collapsed alpha", all_rows, reduced_rows)
        assert closed_projects() == expected_closed, "Minimizing navigation changed folder state"
        toggle_navigation(True)
        assert not find_word(window, "ALPHACLOSED"), "Reopening navigation expanded alpha"
        assert find_word(window, "BETAVISIBLE"), "Reopening navigation collapsed beta"
        assert closed_projects() == expected_closed
        toggle_navigation(False)
        assert compact_icon_rows() == reduced_rows

        process.terminate()
        process.wait(timeout=5)
        process = launch()
        wait_for(lambda: compact_icon_rows() == reduced_rows, "Compact folder state did not survive restart")
        assert closed_projects() == expected_closed
        toggle_navigation(True)
        wait_for(lambda: find_word(window, "BETAVISIBLE"), "Beta did not survive restart")
        assert not find_word(window, "ALPHACLOSED"), "Restart expanded a closed folder"
        # Explicitly expanding the folder still works after both presentations.
        click_nav_label(window, "alpha")
        wait_for(lambda: find_word(window, "ALPHACLOSED"), "Alpha could not be expanded again")
        wait_for(lambda: not closed_projects(), "Explicit expansion was not saved")
        print(f"Navigation collapse smoke passed: icon rows {all_rows} -> {reduced_rows}; reopen/restart preserved")
    except Exception:
        path = tempfile.mktemp(prefix="dish-nav-collapse-failure-", suffix=".png")
        subprocess.run(["import", "-window", "root", path], check=False)
        print(f"Failure screenshot: {path}")
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        log.close()
