"""Details scrolling must not move conversation behind/alongside the pane.

Synthetic Pi, isolated config/home; run under Xvfb after cargo build --release.
Tests both original docked desktop behaviour and narrow overlay, including
non-scrollable header/scrim and wheel chaining at either end of the body.
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


for width in [1240, 780]:
    height = 640
    with tempfile.TemporaryDirectory(prefix="dish-inspector-scroll-") as temporary:
        temp = Path(temporary)
        (temp / "home").mkdir()
        (temp / "config/dish").mkdir(parents=True)
        (temp / "config/dish/state.json").write_text(json.dumps({
            "appearance": "dark", "window_size": [width, height],
            "reduce_motion": True, "details_open": False, "navigation_open": False,
        }))
        session = temp / "history.jsonl"
        session.write_text(json.dumps({"type": "session", "cwd": str(temp), "id": "scroll-fixture"}) + "\n")
        wrapper = temp / "fake-pi"
        wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
        wrapper.chmod(0o700)
        event_path = temp / "events.jsonl"
        env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
                   PI_CODING_AGENT_DIR=str(temp / "agent"),
                   XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
                   DISH_FAKE_PI_LOG=str(event_path), XDG_SESSION_TYPE="x11")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("PI_CODING_AGENT_SESSION_DIR", None)
        log = open(temp / "dish.log", "w+")
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
             "--session", str(session), str(temp)], env=env, stdout=log, stderr=log)
        window = None
        try:
            wait_for(lambda: find_window(), "Window not created")
            window = find_window()
            wait_for(lambda: event_path.exists() and "get_messages" in event_path.read_text(), "Pi did not initialize")
            prepare_window(window)
            wait_for(lambda: find_word(window, "Saved"), "History did not render", timeout=20)
            # Excludes composer/caret, chrome and the rightmost 316px details pane.
            background = f"{width - 460}x260+88+130"
            body = f"260x{height - 180}+{width - 296}+110"

            before = settled(window, background)
            wheel_at(window, width // 3, height // 3, 4, 5)
            wait_for(lambda: pixels(window, background) != before,
                     "Positive control: transcript must really scroll")

            chord("Control_L", "b")
            wait_for(lambda: find_word(window, "Caminho"), "Details did not open")
            still = settled(window, background)
            body_before = settled(window, body)
            wheel_at(window, width - 150, height // 2, 5, 8)
            wait_for(lambda: pixels(window, body) != body_before,
                     "Inspector must handle its own wheel, not swallow it")
            assert settled(window, background) == still, "Inspector body scrolled conversation"

            for button in [4, 5]:
                # First reach the edge, then deliberately overscroll it.
                wheel_at(window, width - 150, height // 2, button, 40)
                assert settled(window, background) == still, "Inspector edge leaked wheel"
                wheel_at(window, width - 150, height // 2, button, 10)
                assert settled(window, background) == still, "Inspector overscroll leaked wheel"
                wheel_at(window, width - 150, 36, button, 8)
                assert settled(window, background) == still, "Inspector header leaked wheel"
                if width < 1120:
                    wheel_at(window, 160, height // 3, button, 8)
                    assert settled(window, background) == still, "Overlay scrim leaked wheel"

            path = tempfile.mktemp(prefix=f"dish-inspector-scroll-{width}-", suffix=".png")
            subprocess.run(["import", "-window", str(window), path], check=True)
            print(f"Inspector scroll smoke passed: {width}px; screenshot: {path}")
        except Exception:
            path = tempfile.mktemp(prefix="dish-inspector-scroll-failure-", suffix=".png")
            subprocess.run(["import", "-window", "root", path], check=False)
            print(f"Failure screenshot: {path}")
            log.flush()
            log.seek(0)
            print(log.read())
            raise
        finally:
            process.terminate()
            process.wait(timeout=5)
            log.close()
