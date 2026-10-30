"""Xvfb regression: query results/selection stay stable during agent streaming."""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-search-stability-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    (temp / "home").mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    archive = sessions / "archive.jsonl"
    records = [{"type": "session", "cwd": str(project), "id": "synthetic-archive"},
               {"type": "session_info", "name": "Archive stable"}]
    records.extend({"type": "message", "message": {"role": "assistant", "content": [
        {"type": "text", "text": f"needle ARCHIVEONLY result {i}"}]}} for i in range(12))
    archive.write_text("\n".join(json.dumps(record) for record in records) + "\n")
    gates = temp / "stream-gates"
    gates.mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(log_path),
               DISH_FAKE_PI_STREAM_GATE_DIR=str(gates), XDG_SESSION_TYPE="x11",
               DISH_TEST_UI_LOG=str(temp / "ui.jsonl"))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    # A aparência segue o desktop por padrão; estes testes verificam o visual
    # escuro, então a preferência fica fixada como numa instalação existente.
    (temp / "config/dish").mkdir(parents=True, exist_ok=True)
    (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))

    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(project)],
                               env=env, stdout=log, stderr=log)

    def events():
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines() if line]

    def deltas():
        return sum(event.get("record", {}).get("type") == "message_update" and
                   event.get("record", {}).get("assistantMessageEvent", {}).get("type") == "text_delta"
                   for event in events())

    window = None
    try:
        wait_for(lambda: find_window(), "Search stability window not created")
        window = find_window()
        UI_TRACES[window] = UiTrace(temp / "ui.jsonl")
        wait_for(lambda: any(event.get("command", {}).get("type") == "get_messages" for event in events()),
                 "Pi did not initialize")
        x11.XSetInputFocus(display, window, 1, 0)

        def composer_ready():
            # Xvfb has no WM; mapping may follow the first title notification.
            expose_window(window)
            node = UI_TRACES[window].node("composer-input")
            return node and visible_point(node)

        def settled_search(query):
            # Paint state, not OCR: one tesseract pass can outlast the whole wait
            # under load, while the panel is already painted (issue #136).
            node = UI_TRACES[window].node("content-search")
            if node and visible_point(node) and node["data"]["query"] == query and not node["data"]["loading"]:
                return node["data"]["hits"]
            return None

        wait_for(composer_ready, "Composer did not render")
        chord("Control_L", "l")
        type_text("long-search-stability")
        press("Return")
        wait_for(lambda: (gates / "ready-0").exists(), "Streaming fixture did not pause")
        chord("Control_L", "Shift_L", "f")
        type_text("needle")
        wait_for(lambda: any("ARCHIVEONLY" in hit["snippet"] for hit in settled_search("needle") or []),
                 "Archived search results missing")
        # Move the pointer without park_mouse(): that helper deliberately uses
        # Ctrl+L, which would move keyboard focus out of the search editor.
        root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        x11.XTranslateCoordinates(display, window, root_window, 5, 5,
                                  ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
        xtst.XTestFakeMotionEvent(display, -1, root_x.value, root_y.value, 0)
        x11.XFlush(display)
        # Assert state, not pixels: anchor_scroll keeps the painted crop a
        # moving target while the text flows (#136). The probe exposes query,
        # loading, hits and the selection, so every claim here reads state.
        def selection_of(id):
            node = UI_TRACES[window].node(id)
            return node["data"]["selected"] if node else None

        press("Tab")  # Keep the first result selected throughout streaming.
        # Tab is picked up on the next painted frame, so wait for the state.
        wait_for(lambda: selection_of("content-search") == 0,
                 "Tab did not select the first result")
        selected = selection_of("content-search")
        assert selected == 0, f"Tab selected {selected!r}, expected the first result"
        expected = wait_for(lambda: settled_search("needle"), "Search results missing after Tab")
        samples = 0
        for index in range(5):
            wait_for(lambda: (gates / f"ready-{index}").exists(), "Next stream delta did not pause")
            (gates / f"release-{index}").touch()
            wait_for(lambda: deltas() >= index + 1, "Stream delta was not emitted")
            deadline = time.monotonic() + 0.4
            while time.monotonic() < deadline:
                assert settled_search("needle") == expected, "Search results rebuilt during streaming"
                assert selection_of("content-search") == 0, "Search selection lost during streaming"
                samples += 1
        wait_for(lambda: any(event.get("record", {}).get("type") == "agent_end" for event in events()),
                 "Streaming agent did not finish")
        assert settled_search("needle") == expected, "Agent completion refreshed the open search"
        assert selection_of("content-search") == 0, "Search selection lost after completion"
        # An actual edit must still refresh the snapshot.
        chord("Control_L", "a")
        type_text("no-such-content")
        wait_for(lambda: (hits := settled_search("no-such-content")) is not None and
                 not any("ARCHIVEONLY" in hit["snippet"] for hit in hits),
                 "Editing the query did not replace results")
        press("Escape")
        assert process.poll() is None, (temp / "dish.log").read_text()
        print(f"Search stability smoke passed: {samples} state samples across streaming, completion and query edits")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-search-stability-failure-", suffix=".png")
        subprocess.run(["import", "-window", str(window) if window else "root", screenshot], check=False)
        print("Search stability failure screenshot:", screenshot)
        if window:
            save_ui_trace(window, "dish-search-stability")
        print(log_path.read_text() if log_path.exists() else "No fixture events")
        print((temp / "dish.log").read_text())
        raise
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        log.close()
