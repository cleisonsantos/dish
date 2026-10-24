"""Xvfb regressions: overlay wheel isolation and long keyboard-select dialogs.

Synthetic Pi, temporary home/config. Exercise both desktop and minimum-size
windows. A positive control proves the background history can really scroll;
inner scrolling must work while scrims, headers and edges isolate that history.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


def pixels(window, crop):
    return subprocess.check_output([
        "import", "-window", str(window), "-crop", crop, "-depth", "8", "rgb:-"],
        stderr=subprocess.DEVNULL)


def settled(window, crop):
    previous = [pixels(window, crop)]

    def stable():
        current = pixels(window, crop)
        same = current == previous[0]
        previous[0] = current
        return same

    wait_for(stable, "Pixels did not settle")
    return previous[0]


for width, height in [(1240, 640), (720, 520)]:
    with tempfile.TemporaryDirectory(prefix="dish-dialog-scroll-") as temporary:
        temp = Path(temporary)
        (temp / "home").mkdir()
        (temp / "config/dish").mkdir(parents=True)
        (temp / "gate").mkdir()
        (temp / "config/dish/state.json").write_text(json.dumps({
            "appearance": "dark", "window_size": [width, height],
            "reduce_motion": True, "details_open": False, "navigation_open": False,
        }))
        session = temp / "history.jsonl"
        session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": "dialog-fixture"}) + "\n")
        wrapper = temp / "fake-pi"
        wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
        wrapper.chmod(0o700)
        event_path = temp / "events.jsonl"
        env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
                   PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
                   XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(event_path),
                   DISH_FAKE_PI_RUN_GATE_DIR=str(temp / "gate"), XDG_SESSION_TYPE="x11")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("PI_CODING_AGENT_SESSION_DIR", None)

        def events():
            result = []
            if event_path.exists():
                for line in event_path.read_text().splitlines():
                    try:
                        result.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass
            return result

        def commands(kind):
            return [item["command"] for item in events()
                    if item.get("command", {}).get("type") == kind]

        def ended(message):
            return any(message in str(item.get("record", {}))
                       and item.get("record", {}).get("type") == "message_end"
                       and item["record"].get("message", {}).get("role") == "assistant"
                       for item in events())

        def screenshot(label):
            path = tempfile.mktemp(prefix=f"dish-dialog-{width}-{label}-", suffix=".png")
            subprocess.run(["import", "-window", str(window) if window else "root", path], check=True)
            print(f"{label}: {path}")

        log = open(temp / "dish.log", "w+")
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
             "--session", str(session), str(temp)], env=env, stdout=log, stderr=log)
        window = None
        try:
            wait_for(lambda: find_window(), "Window not created")
            window = find_window()
            wait_for(lambda: commands("get_messages"), "Pi did not initialize")
            prepare_window(window)
            wait_for(lambda: find_word(window, "Saved"), "History did not render")
            history = f"{width - 100}x140+60+100"
            before = settled(window, history)
            wheel_at(window, width // 3, height // 3, 4, 5)
            wait_for(lambda: pixels(window, history) != before,
                     "Positive control: history did not scroll")

            # Check each direction separately: up/down must not accidentally
            # cancel a leaked scroll before the background comparison.
            for button in [5, 4]:
                park_mouse(window)
                before_settings = settled(window, history)
                press("F1")
                wait_for(lambda: find_word(window, "Fechar"), "Settings did not open")
                close = find_word(window, "Fechar")
                position = (close[0] - 16, height - 110)
                if button == 5:
                    wait_for(lambda: find_word(window, "Buscar"), "Shortcut search did not render")
                    search = find_word(window, "Buscar")
                    wheel_at(window, *position, 5, 0)
                    settings_body = f"{max(120, close[0] - search[0] - 32)}x90+{search[0]}+{position[1] - 40}"
                    settings_before = settled(window, settings_body)
                    wheel_at(window, *position, 5, 12)
                    wait_for(lambda: pixels(window, settings_body) != settings_before,
                             "Settings content did not scroll")
                wheel_at(window, *position, button, 40)
                # Same point as the positive control, now behind settings.
                # Footer/header-only probes can miss the history's hitbox.
                wheel_at(window, width // 3, height // 3, button, 8)
                wheel_at(window, close[0], close[1], button, 8)
                wheel_at(window, 60, height - 30, button, 8)
                press("Escape")
                park_mouse(window)
                assert settled(window, history) == before_settings, "Settings scrim leaked wheel to history"

            # Long title/message/list must fit. Arrows reveal both endpoints,
            # including wraparound, without submitting a new composer prompt.
            chord("Control_L", "l")
            type_text("select-many")
            press("Return")
            wait_for(lambda: find_word(window, "PRIMEIRA"), "First selection option is not visible")
            assert not find_word(window, "ULTIMA"), "Long option list was squeezed into the viewport"
            title = find_word(window, "Escolher")
            assert title and title[1] >= 16, "Dialog heading was clipped above the window"
            card_left = (width + 44) // 2 - 230
            background = f"80x140+{max(60, card_left - 100)}+100"
            modal_background = settled(window, background)
            first = find_word(window, "PRIMEIRA")
            wheel_at(window, first[0] + 40, first[1] + 3, 5, 8)
            wait_for(lambda: not find_word(window, "PRIMEIRA"), "Options did not handle their own wheel")
            for button in [4, 5]:
                wheel_at(window, first[0] + 40, first[1] + 3, button, 80)
                wheel_at(window, title[0], title[1] + title[3] // 2, button, 8)
                wheel_at(window, max(60, card_left - 40), height // 2, button, 8)
                assert settled(window, background) == modal_background, "Selection dialog leaked wheel"
            press("Up")
            wait_for(lambda: find_word(window, "ULTIMA"), "Keyboard highlight did not reveal last option")
            last = find_word(window, "ULTIMA")
            assert last[1] + last[3] < height - 16, "Last option was clipped below the window"
            assert not commands("extension_ui_response"), "Arrows unexpectedly submitted the dialog"
            press("Down")
            wait_for(lambda: find_word(window, "PRIMEIRA"), "Wraparound did not reveal first option")
            press("Up")
            wait_for(lambda: find_word(window, "ULTIMA"), "Last option did not return")
            screenshot("last-option")
            press("Return")
            wait_for(lambda: commands("extension_ui_response"), "Selection did not submit")
            response = commands("extension_ui_response")[-1]
            assert response.get("id") == "fake-select-many" and response.get("value") == "ULTIMA alternativa", response
            assert len(commands("prompt")) == 1, "Dialog keyboard input leaked into composer"
            wait_for(lambda: ended("select-many"), "Synthetic select run did not finish")

            # Keep another run open deterministically while testing close scrim.
            chord("Control_L", "l")
            type_text("long-activity-close")
            press("Return")
            wait_for(lambda: any(item.get("record", {}).get("assistantMessageEvent", {}).get("delta", "")
                                 .startswith("long-activity-close")
                                 and item["record"]["assistantMessageEvent"]["delta"].endswith("/ 4\n")
                                 for item in events()), "Close fixture did not reach completion gate")
            for button in [4, 5]:
                park_mouse(window)
                before_close = settled(window, history)
                request_window_close(window)
                wait_for(lambda: find_word(window, "Tarefas"), "Close confirmation did not open")
                wheel_at(window, width // 2, height // 2, button, 12)
                wheel_at(window, 60, height - 30, button, 12)
                press("Escape")
                park_mouse(window)
                assert process.poll() is None, "Canceling confirmation closed the app"
                assert settled(window, history) == before_close, "Close confirmation leaked wheel"
                assert not commands("abort"), "Canceling confirmation aborted the agent"
            (temp / "gate/long-activity-close").write_text("finish")
            print(f"Dialog scroll smoke passed: {width}x{height}")
        except Exception:
            screenshot("failure")
            log.flush()
            log.seek(0)
            print(log.read())
            raise
        finally:
            process.terminate()
            process.wait(timeout=5)
            log.close()
