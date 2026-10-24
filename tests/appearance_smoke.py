"""Xvfb: all built-in themes apply, scroll into view, and survive a restart.

Synthetic Pi, isolated home/config. Locate controls by rendered text, not fixed
screen coordinates. Check dominant surface pixels as well as saved preferences,
so choosing an id without repainting the palette cannot pass.
"""
from pathlib import Path
import collections
import unicodedata

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

THEMES = [
    ("Grafite", "dish-dark", True, (23, 27, 33)),
    ("Grafite baixo", "dish-dim", True, (28, 31, 37)),
    ("Noite azul", "dish-midnight", True, (17, 27, 41)),
    ("Ameixa", "dish-plum", True, (28, 24, 32)),
    ("Papel frio", "dish-light", False, (233, 237, 243)),
    ("Papel quente", "dish-paper", False, (250, 247, 240)),
    ("Lavanda", "dish-lavender", False, (235, 231, 242)),
    ("Sálvia", "dish-sage", False, (230, 238, 232)),
]


def normalized(value):
    return "".join(c for c in unicodedata.normalize("NFKD", value.casefold())
                   if c.isalnum() or c.isspace()).strip()


with tempfile.TemporaryDirectory(prefix="dish-appearance-") as temporary:
    temp = Path(temporary)
    (temp / "home").mkdir()
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    config = temp / "config/dish"
    config.mkdir(parents=True)
    state_path = config / "state.json"
    state_path.write_text(json.dumps({"appearance": "dark", "reduce_motion": True}))
    (config / "themes").mkdir()
    (config / "themes/meu-tema.json").write_text(json.dumps({
        "name": "Meu tema", "appearance": "light",
        "colors": {"surface": "#faf7f0", "bg": "#f5f1e7", "surface2": "#fffdf9"},
    }))
    env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(temp / "agent"), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_LOG=str(log_path),
               XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    screenshot = tempfile.mktemp(prefix="dish-appearance-", suffix=".png")
    log = open(temp / "dish.log", "w+")
    window = None
    process = None

    def saved():
        return json.loads(state_path.read_text())

    def initialized_runs():
        if not log_path.exists():
            return 0
        count = 0
        for line in log_path.read_text().splitlines():
            try:
                count += json.loads(line).get("command", {}).get("type") == "get_messages"
            except json.JSONDecodeError:
                pass  # A final JSONL frame may still be in flight.
        return count

    def launch():
        global window
        previous = initialized_runs()
        child = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(temp)],
            env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: initialized_runs() > previous, "Synthetic Pi did not initialize")
            wait_for(lambda: find_window(), "Window not created")
            window = find_window()
            prepare_window(window)
            wait_for(lambda: find_word(window, "Pergunte"), "Composer did not render")
        except Exception:
            child.terminate()
            child.wait(timeout=5)
            raise
        return child

    def capture():
        subprocess.run(["import", "-window", str(window), screenshot], check=True)

    def text_box(label, near_y=None, exact_case=False):
        """Exact word sequences; accent-insensitive. Retry OCR at 2x if needed."""
        capture()
        wanted = normalized(label).split()
        for scale, psm in [(1, 11), (1, 6), (2, 6)]:
            image = screenshot
            if scale != 1:
                image = str(temp / "large.png")
                subprocess.run(["convert", screenshot, "-resize", "200%", image],
                               check=True, stderr=subprocess.DEVNULL)
            rows = subprocess.check_output(
                ["tesseract", image, "stdout", "--psm", str(psm), "tsv"],
                stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            lines = collections.defaultdict(list)
            for row in rows:
                fields = row.split("\t")
                if len(fields) == 12 and fields[11].strip():
                    lines[tuple(fields[1:5])].append(fields)
            for words in lines.values():
                texts = [normalized(word[11]) for word in words]
                for index in range(len(words) - len(wanted) + 1):
                    if texts[index:index + len(wanted)] != wanted:
                        continue
                    if wanted == ["grafite"] and texts[index + 1:index + 2] == ["baixo"]:
                        continue
                    matched = words[index:index + len(wanted)]
                    if exact_case and [word[11].strip() for word in matched] != label.split():
                        continue
                    x = min(int(w[6]) for w in matched) // scale
                    y = min(int(w[7]) for w in matched) // scale
                    right = max(int(w[6]) + int(w[8]) for w in matched) // scale
                    bottom = max(int(w[7]) + int(w[9]) for w in matched) // scale
                    if near_y is None or abs((y + bottom) // 2 - near_y) < 14:
                        return x, y, right - x, bottom - y
        return None

    def activate(box):
        click(window, box[0] + box[2] // 2, box[1] + box[3] // 2)

    def open_appearance():
        press("F1")
        wait_for(lambda: find_word(window, "Apar"), "Appearance section missing")
        box = find_word(window, "Apar")
        activate(box)
        wait_for(lambda: text_box("Tema", exact_case=True), "Appearance page did not render")

    def content_scroll(button, count):
        close = text_box("Fechar")
        assert close, "Settings header missing"
        attributes = XWindowAttributes()
        assert x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
        # Inside the content column, anchored by the close control on its right.
        wheel_at(window, close[0] - 24, attributes.height // 2, button, count)
        previous = [None]

        def settled():
            # Repeated wheel events can still be awaiting a frame when OCR
            # starts. Wait for layout before reusing a row's text coordinates.
            crop = f"600x450+{max(0, close[0] - 550)}+200"
            current = subprocess.check_output(
                ["import", "-window", str(window), "-crop", crop, "-depth", "8", "rgb:-"],
                stderr=subprocess.DEVNULL)
            same = current == previous[0]
            previous[0] = current
            return same

        wait_for(settled, "Theme catalog scroll did not settle")

    def select_mode(label, mode):
        # The explanatory paragraph also says 'Sistema'; anchor the actual
        # segmented control to its row label, not that earlier occurrence.
        row = text_box("Tema", exact_case=True)
        assert row, "Default-theme row missing"
        box = text_box(label, row[1] + row[3] // 2)
        assert box, f"Mode segment missing: {label}"
        activate(box)
        wait_for(lambda: saved().get("appearance") == mode and saved().get("theme") is None,
                 f"Mode {mode} was not applied/saved")

    def surface_visible(color):
        capture()
        raw = subprocess.check_output(
            ["convert", screenshot, "-depth", "8", "rgb:-"], stderr=subprocess.DEVNULL)
        # Tiny palette swatches alone must not satisfy the repaint assertion.
        return raw.count(bytes(color)) * 3 > len(raw) * 0.08

    def select_theme(label, theme_id, dark, color):
        content_scroll(4, 40)
        box = None
        for _ in range(12):
            box = text_box(label)
            if box:
                break
            content_scroll(5, 3)
        assert box, f"Theme not reachable by scrolling: {label}"
        activate(box)
        wait_for(lambda: saved().get("theme") == theme_id, f"Theme id not saved: {theme_id}")
        assert saved().get("appearance") == ("dark" if dark else "light")
        wait_for(lambda: surface_visible(color), f"Theme palette did not repaint: {theme_id}")
        print(f"Theme applied: {theme_id}")

    try:
        process = launch()
        open_appearance()
        select_mode("Claro", "light")
        select_mode("Escuro", "dark")
        for item in THEMES:
            select_theme(*item)
        assert saved().get("theme") == "dish-sage"
        process.terminate()
        process.wait(timeout=5)
        process = launch()
        open_appearance()
        wait_for(lambda: surface_visible((230, 238, 232)), "Sage did not survive restart")
        select_theme("Meu tema", "user:meu-tema", False, (250, 247, 240))
        process.terminate()
        process.wait(timeout=5)
        process = launch()
        open_appearance()
        assert saved().get("theme") == "user:meu-tema"
        wait_for(lambda: surface_visible((250, 247, 240)), "User theme did not survive restart")
        print(f"Appearance smoke passed: 8 built-ins + user theme; screenshot: {screenshot}")
    except Exception:
        if window:
            capture()
            print(f"Appearance failure screenshot: {screenshot}")
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        if process and process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        log.close()
