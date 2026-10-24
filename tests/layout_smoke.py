"""Wide chat measure and rounded paint-owner regressions (synthetic Pi only).

Run under Xvfb with a screen at least 2000x1120 after cargo build --release.
Text anchors locate content; pixel probes derive frame bounds from its paint,
not from assumed navigation widths. Both palettes and inspector modes covered.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

PALETTES = {
    "dark": {"canvas": (34, 39, 47), "outer": (23, 27, 33),
             "writing": (43, 50, 60), "border": (85, 96, 112)},
    "light": {"canvas": (247, 249, 252), "outer": (233, 237, 243),
              "writing": (255, 255, 255), "border": (195, 203, 213)},
}


for width, height, appearance in [(1600, 960, "dark"), (1920, 1080, "light"),
                                  (780, 760, "dark"), (720, 520, "light")]:
    with tempfile.TemporaryDirectory(prefix="dish-layout-") as temporary:
        temp = Path(temporary)
        project = temp / "projeto-sintetico"
        project.mkdir()
        (temp / "home").mkdir()
        config = temp / "config/dish"
        config.mkdir(parents=True)
        (config / "state.json").write_text(json.dumps({
            "appearance": appearance, "window_size": [width, height],
            "reduce_motion": True, "details_open": False, "navigation_open": False,
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
        screenshot = Path(tempfile.mktemp(prefix=f"dish-layout-{width}-{appearance}-", suffix=".png"))
        palette = PALETTES[appearance]
        window = None

        def capture():
            subprocess.run(["import", "-window", str(window), str(screenshot)], check=True)
            dimensions = tuple(map(int, subprocess.check_output(
                ["identify", "-format", "%w %h", str(screenshot)]).split()))
            assert dimensions == (width, height), ("Unexpected client dimensions", dimensions)
            raw = subprocess.check_output(["convert", str(screenshot), "-depth", "8", "rgb:-"])
            assert len(raw) == width * height * 3
            return raw

        def pixel(raw, x, y):
            offset = (y * width + x) * 3
            return tuple(raw[offset:offset + 3])

        def painted_bounds(raw):
            # The blank middle of the welcome screen spans the whole canvas.
            xs = [x for x in range(width) if pixel(raw, x, height // 2) == palette["canvas"]]
            assert xs, "Canvas palette did not render"
            left, right = min(xs), max(xs)
            ys = [y for y in range(height) if pixel(raw, width // 2, y) == palette["canvas"]]
            return left, min(ys), right, max(ys)

        def check_composer(raw, bounds):
            left, _, right, _ = bounds
            editor = find_word(window, "Pergunte")
            assert editor, "Composer placeholder missing"
            y = editor[1] + editor[3] // 2
            writing = [x for x in range(left, right + 1)
                       if pixel(raw, x, y) == palette["writing"]]
            assert writing, "Composer paint not found"
            assert 15 <= min(writing) - left <= 19, ("Excess composer left gutter", min(writing), bounds)
            assert 15 <= right - max(writing) <= 19, ("Excess composer right gutter", max(writing), bounds)
            assert max(writing) - min(writing) > right - left - 42, "Composer still width-capped"
            return editor

        def check_corners(raw, bounds):
            left, top, right, bottom = bounds
            for x, y in [(left + 1, top + 1), (right - 1, top + 1),
                         (left + 1, bottom - 1), (right - 1, bottom - 1)]:
                assert pixel(raw, x, y) == palette["outer"], (
                    "Square descendant covered rounded cutout", (x, y), pixel(raw, x, y))

        with open(temp / "dish.log", "w+") as log:
            process = subprocess.Popen(
                [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(project)],
                env=env, stdout=log, stderr=log)
            try:
                wait_for(lambda: find_window(), "Window not created")
                window = find_window()
                prepare_window(window)
                wait_for(lambda: event_path.exists() and "get_messages" in event_path.read_text(),
                         "Synthetic Pi did not initialize")
                wait_for(lambda: find_word(window, "Qual"), "Welcome did not render")
                wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
                raw = capture()
                bounds = painted_bounds(raw)
                left, top, right, bottom = bounds
                check_corners(raw, bounds)

                welcome = find_word(window, "Qual")
                assert 12 <= welcome[0] - left <= 24, ("Welcome retained centered gutter", welcome, bounds)
                editor = check_composer(raw, bounds)

                # Border arc must survive opaque inspector paint in both modes.
                # Outside the inner 13px curve (including its AA fringe),
                # but inside the outer border's antialiased 14px curve.
                arc = (right - 3, top + 2)
                border_pixel = pixel(raw, *arc)
                assert border_pixel != palette["outer"], ("Positive control: missing border arc", arc)
                chord("Control_L", "b")
                wait_for(lambda: find_word(window, "Caminho"), "Inspector did not open")
                details = capture()
                assert pixel(details, *arc) == border_pixel, (
                    "Inspector covered curved border", arc, border_pixel, pixel(details, *arc))
                check_corners(details, bounds)
                chord("Control_L", "b")
                wait_for(lambda: not find_word(window, "Caminho"), "Inspector did not close")

                # Actual assistant output, not only empty state, uses the inset.
                click(window, editor[0] + 12, editor[1] + editor[3] // 2)
                type_text("layout-probe")
                press("Return")
                wait_for(lambda: find_word(window, "Finished"), "Assistant output did not render")
                answer = find_word(window, "Finished")
                assert 12 <= answer[0] - left <= 24, ("Assistant retained centered gutter", answer, bounds)
                check_corners(capture(), bounds)

                if width == 1600:
                    # Removing the writing cap must not make popups 1500px wide.
                    def check_popup(word, maximum):
                        box = find_word(window, word)
                        raw = capture()
                        y = box[1] + box[3] // 2
                        # Measure opaque border, not shadow pixels that can
                        # accidentally match the popup's background color.
                        painted = [x for x in range(left, right + 1)
                                   if pixel(raw, x, y) == palette["border"]]
                        assert len(painted) >= 2, ("Popup border missing", word)
                        assert 15 <= min(painted) - left <= 19, ("Popup misaligned", word, min(painted))
                        assert maximum - 5 <= max(painted) - min(painted) <= maximum, (
                            "Popup inherited full chat width", word, max(painted) - min(painted))

                    low = find_word(window, "baixo")
                    click(window, low[0] + low[2] // 2, low[1] + low[3] // 2)
                    wait_for(lambda: find_word(window, "alto"), "Effort popup did not open")
                    check_popup("alto", 360)
                    press("Escape")
                    chord("Control_L", "Shift_L", "m")
                    wait_for(lambda: find_word(window, "Selecionar"), "Model picker did not open")
                    check_popup("Selecionar", 640)
                    press("Escape")

                if width >= 1600:
                    # A left-aligned 760px reply cap would pass the short test.
                    # Put a sentinel beyond that cap, then require a wrapped tail.
                    long_request = "largura " * 20 + "ALCANCE " + "revisar " * 30 + "FIMTEXTO"
                    type_text(long_request)
                    press("Return")

                    def long_reply_ready():
                        user = find_word(window, "Voc", last=True)
                        reply = find_word(window, "Finished", last=True)
                        return user and reply and reply[1] > user[1]

                    wait_for(long_reply_ready, "Long assistant output did not render")
                    reach = find_word(window, "ALCANCE", last=True)
                    tail = find_word(window, "FIMTEXTO", last=True)
                    reply = find_word(window, "Finished", last=True)
                    assert reach and reach[0] > left + 760, ("Reply retained old width cap", reach)
                    assert tail and tail[1] > reply[1], ("Reply tail clipped instead of wrapping", tail)
                    assert tail[0] + tail[2] <= right - 12, ("Wrapped tail escaped canvas", tail)
                    user = find_word(window, "Voc", last=True)
                    raw = capture()
                    y = user[1] + user[3] // 2
                    bubble = [x for x in range(left, right + 1)
                              if pixel(raw, x, y) == palette["writing"]]
                    assert bubble, "Long request bubble not painted"
                    available = right - left - 32
                    fraction = (max(bubble) - min(bubble)) / available
                    assert 0.87 <= fraction <= 0.905, ("Request must use adaptive 90% bound", fraction)
                    assert 15 <= right - max(bubble) <= 19, "Request not right aligned"

                # Expanded navigation + docked details shrink the actual writing
                # space; minimum-window controls must fit this space as well.
                chord("Control_L", "Shift_L", "b")
                wait_for(lambda: json.loads((config / "state.json").read_text()).get("navigation_open"),
                         "Navigation did not expand")
                if width >= 1120:
                    chord("Control_L", "b")
                    wait_for(lambda: find_word(window, "Caminho"), "Docked details did not open")
                wait_for(lambda: find_word(window, "Detalhes"), "Header controls clipped after navigation expansion")
                raw = capture()
                check_composer(raw, painted_bounds(raw))
                print(f"Layout smoke passed: {width}x{height}, {appearance}; screenshot: {screenshot}")
            except Exception:
                if window:
                    subprocess.run(["import", "-window", str(window), str(screenshot)], check=False)
                print(f"Failure screenshot: {screenshot}")
                # Runtime logs intentionally stay private: diagnostics may contain data.
                raise
            finally:
                process.terminate()
                process.wait(timeout=5)
