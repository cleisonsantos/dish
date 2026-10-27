"""Global notification FIFO, persistence and streaming gate with synthetic Pi.

DISH_TEST_NOTIFY points at a JSON array of {id,title,detail}. The app polls it
at runtime and writes the accepted ids to the sibling .ack file for a race-free
smoke. This hook does not add product UI.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-notifications-") as temporary:
    temp = Path(temporary)
    project = temp / "project"
    project.mkdir()
    agent = temp / "agent"
    agent.mkdir()
    gates = temp / "gates"
    gates.mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    feed = temp / "notifications.json"
    feed.write_text("[]")
    ack = feed.with_suffix(".ack")
    state = temp / "config/dish/state.json"
    state.parent.mkdir(parents=True)
    state.write_text(json.dumps({"appearance": "dark"}))
    events_path = temp / "events.jsonl"
    launch_count = 0
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(agent),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(events_path), DISH_FAKE_PI_RUN_GATE_DIR=str(gates),
               DISH_TEST_NOTIFY=str(feed), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = (temp / "dish.log").open("w")
    process = None

    def records():
        if not events_path.exists():
            return []
        result = []
        for line in events_path.read_text().splitlines():
            try:
                result.append(json.loads(line))
            except json.JSONDecodeError:
                pass
        return result

    def prompts():
        return [row for row in records() if row.get("command", {}).get("type") == "prompt"]

    def notification(identity):
        return UI_TRACES[window].node(f"notification-{identity}")

    def shown(identity):
        node = notification(identity)
        return node and visible_point(node)

    def update_feed(items):
        replacement = feed.with_suffix(".next")
        replacement.write_text(json.dumps(items))
        replacement.replace(feed)
        expected = "\n".join(item["id"] for item in items)
        wait_for(lambda: ack.exists() and ack.read_text() == expected,
                 "Notification feed was not consumed")

    def launch():
        global process, window, launch_count
        launch_count += 1
        trace_path = temp / f"ui-{launch_count}.jsonl"
        trace_path.unlink(missing_ok=True)
        env["DISH_TEST_UI_LOG"] = str(trace_path)
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
                                    str(project)], env=env, stdout=log, stderr=log)
        window = wait_for(find_window, "Dish window was not created")
        prepare_window(window)
        wait_for(lambda: trace_path.exists() and trace_path.stat().st_size > 0,
                 f"Trace was not created on launch {launch_count}")
        UI_TRACES[window] = UiTrace(trace_path)
        wait_for(lambda: UI_TRACES[window].node("frame"), "Workspace did not paint")

    def stop():
        process.terminate()
        process.wait(timeout=10)
        UI_TRACES.pop(window, None)
        wait_for(lambda: not find_window(), "Dish window did not close")

    def item(identity):
        number = {"one": "um", "two": "dois", "three": "três", "during": "durante"}[identity]
        return {"id": identity, "title": f"Aviso {number}", "detail": f"Detalhe {number}"}

    try:
        launch()
        chord("Control_L", "l")
        update_feed([item("one"), item("two"), item("three")])
        wait_for(lambda: shown("one") and shown("two"), "First two notifications did not appear")
        assert not notification("three"), "FIFO overflow became visible before dismissal"
        assert notification("one")["visible"][1] < notification("two")["visible"][1], "FIFO visual order changed"
        type_text("focus")
        wait_for(lambda: UI_TRACES[window].node("frame")["data"]["draft"] == "focus",
                 "Notification stole keyboard focus from the composer")
        chord("Control_L", "a")
        press("BackSpace")
        assert find_word(window, "Aviso", crop=(800, 0, 420, 300)), "Notification was not visible on screen"
        click_ui(window, "notification-dismiss-one")
        wait_for(lambda: "one" in json.loads(state.read_text()).get("dismissed_notifications", []),
                 "Dismissal was not persisted to state.json")
        wait_for(lambda: shown("three") and not notification("one"), "FIFO did not advance")
        stop()
        launch()
        wait_for(lambda: shown("two") and shown("three"), "Notifications did not reload in FIFO order")
        assert not notification("one"), "Dismissed notification reappeared after restart"
        update_feed([item("one")])
        # Existing cards remain queued by design; clear them through their controls.
        for identity in ("two", "three"):
            click_ui(window, f"notification-dismiss-{identity}")
            wait_for(lambda: not notification(identity), f"Could not dismiss {identity}")
        wait_for(lambda: set(json.loads(state.read_text()).get("dismissed_notifications", []))
                 == {"one", "two", "three"}, "Dismissals were not persisted to state.json")
        before = len(prompts())
        # Após o relaunch, o primeiro chord não garante foco interno no Xvfb;
        # clicar no composer reproduz o gesto do usuário.
        click_ui(window, "composer-input")
        time.sleep(0.3)
        chord("Control_L", "l")
        type_text("long-activity-notify")
        press("Return")
        wait_for(lambda: len(prompts()) > before, "Streaming prompt was not sent")
        wait_for(lambda: UI_TRACES[window].node("frame")["data"]["executing"],
                 "Workspace did not enter streaming state")
        update_feed([item("one"), item("during")])
        generation = UI_TRACES[window].generation
        wait_for(lambda: UI_TRACES[window].poll() is not None and UI_TRACES[window].generation > generation,
                 "No paint after notification injection during streaming")
        assert not notification("during"), "Notification appeared during streaming"
        (gates / "long-activity-notify").touch()
        wait_for(lambda: shown("during"), "Queued notification did not appear after streaming")
        dismissed = json.loads(state.read_text())["dismissed_notifications"]
        assert set(dismissed) == {"one", "two", "three"}, "Persisted dismissals changed after streaming"
        print(f"state.json dismissed_notifications: {json.dumps(dismissed)}")
        print("Notification smoke passed: display, FIFO, persistence, restart and streaming gate")
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        log.close()
