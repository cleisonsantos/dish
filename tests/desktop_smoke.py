"""Run with xvfb-run -a python3 tests/desktop_smoke.py after cargo build.

Uses XTest, an isolated fake Pi, and temporary projects; never opens user sessions.
"""
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from ui_test_support import UiTrace, parse_words, visible_point, visual_lines

UI_TRACES = {}
ROOT = Path(__file__).resolve().parents[1]
x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
xtst = ctypes.CDLL(ctypes.util.find_library("Xtst"))
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x11.XDefaultRootWindow.restype = ctypes.c_ulong
x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
x11.XStringToKeysym.restype = ctypes.c_ulong
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XKeysymToKeycode.restype = ctypes.c_uint
x11.XFlush.argtypes = [ctypes.c_void_p]


class XWindowAttributes(ctypes.Structure):
    _fields_ = [
        ("x", ctypes.c_int), ("y", ctypes.c_int),
        ("width", ctypes.c_int), ("height", ctypes.c_int),
        ("border_width", ctypes.c_int), ("depth", ctypes.c_int),
        ("visual", ctypes.c_void_p), ("root", ctypes.c_ulong),
        ("window_class", ctypes.c_int), ("bit_gravity", ctypes.c_int),
        ("win_gravity", ctypes.c_int), ("backing_store", ctypes.c_int),
        ("backing_planes", ctypes.c_ulong), ("backing_pixel", ctypes.c_ulong),
        ("save_under", ctypes.c_int), ("colormap", ctypes.c_ulong),
        ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int),
        ("all_event_masks", ctypes.c_long), ("your_event_mask", ctypes.c_long),
        ("do_not_propagate_mask", ctypes.c_long), ("override_redirect", ctypes.c_int),
        ("screen", ctypes.c_void_p),
    ]


x11.XGetWindowAttributes.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(XWindowAttributes)]
x11.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                          ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
                          ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
x11.XFetchName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_char_p)]
x11.XFree.argtypes = [ctypes.c_void_p]
x11.XTranslateCoordinates.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
                                     ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_int),
                                     ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong)]
x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
x11.XClearArea.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int,
                          ctypes.c_uint, ctypes.c_uint, ctypes.c_int]
x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x11.XInternAtom.restype = ctypes.c_ulong
x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.c_void_p]
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
display = x11.XOpenDisplay(None)
assert display, "An X11 display is required"
root_window = x11.XDefaultRootWindow(display)


def find_window(parent=root_window):
    name = ctypes.c_char_p()
    if x11.XFetchName(display, parent, ctypes.byref(name)) and name.value:
        title = name.value.decode()
        x11.XFree(name)
        if title == "Dish":
            return parent
    root, ancestor = ctypes.c_ulong(), ctypes.c_ulong()
    children = ctypes.POINTER(ctypes.c_ulong)()
    count = ctypes.c_uint()
    x11.XQueryTree(display, parent, ctypes.byref(root), ctypes.byref(ancestor),
                   ctypes.byref(children), ctypes.byref(count))
    child_ids = [children[i] for i in range(count.value)]
    if children:
        x11.XFree(children)
    for child in child_ids:
        found = find_window(child)
        if found:
            return found
    return None


def wait_for(predicate, description, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError(description)


def prepare_window(window):
    """RPC and WM_NAME can arrive before mapping; avoid early BadMatch/expose."""
    def viewable():
        attributes = XWindowAttributes()
        return x11.XGetWindowAttributes(display, window, ctypes.byref(attributes)) and attributes.map_state == 2
    wait_for(viewable, "Window did not become viewable")
    x11.XSetInputFocus(display, window, 1, 0)
    expose_window(window)


def expose_window(window):
    # Xvfb has no window manager; request the expose a real desktop sends when
    # a newly mapped window becomes visible. Do not clear during pixel checks.
    x11.XClearArea(display, window, 0, 0, 0, 0, True)
    x11.XFlush(display)


def key(name, pressed):
    code = x11.XKeysymToKeycode(display, x11.XStringToKeysym(name.encode()))
    assert code, name
    xtst.XTestFakeKeyEvent(display, code, pressed, 0)
    x11.XFlush(display)


def press(name):
    key(name, True)
    key(name, False)


def type_text(text):
    for char in text:
        press({" ": "space", "-": "minus", "/": "slash"}.get(char, char))
    time.sleep(0.1)


def visible_accent_pixels(window):
    # Effort is text now, not a solid dot. Include the composer regardless of
    # navigation/details width; never require a decorative marker for this test.
    attributes = XWindowAttributes()
    assert x11.XGetWindowAttributes(display, window, ctypes.byref(attributes))
    height = max(1, attributes.height // 4)
    crop = f"{attributes.width}x{height}+0+{attributes.height - height}"
    return subprocess.check_output(
        ["import", "-window", str(window), "-crop", crop, "-depth", "8", "rgb:-"])


def contains_rendered_color(pixels, expected, tolerance=8):
    # Font rasterization can blend even the most opaque stroke with its surface.
    return any(all(abs(channel - target) <= tolerance for channel, target in zip(pixel, expected))
               for pixel in zip(pixels[0::3], pixels[1::3], pixels[2::3]))


def click(window, x, y):
    root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
    x11.XTranslateCoordinates(display, window, root_window, 0, 0,
                              ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
    xtst.XTestFakeMotionEvent(display, -1, root_x.value + x, root_y.value + y, 0)
    xtst.XTestFakeButtonEvent(display, 1, True, 0)
    xtst.XTestFakeButtonEvent(display, 1, False, 0)
    x11.XFlush(display)
    time.sleep(0.2)


def find_word(window, needle, last=False, crop=None):
    """OCR only for visual checks/selection. Prefer paint state for app state.

    Crop before enlarging: unrelated columns confuse segmentation. Return words
    in visual Y/X order, never tesseract's block/paragraph/line ordering.
    """
    origin = crop[:2] if crop else (0, 0)
    with tempfile.TemporaryDirectory(prefix="dish-ocr-") as directory:
        image = str(Path(directory) / "crop.png")
        args = ["import", "-window", str(window)]
        if crop:
            x, y, width, height = crop
            args += ["-crop", f"{width}x{height}+{x}+{y}"]
        capture = subprocess.run(args + ["-resize", "200%", image],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if capture.returncode != 0:
            return None  # Window may have its title before it is mapped.
        for psm in ("11", "6"):
            tsv = subprocess.check_output(
                ["tesseract", image, "stdout", "--psm", psm, "tsv"],
                stderr=subprocess.DEVNULL).decode()
            lines = visual_lines(parse_words(tsv, origin=origin))
            matches = [tuple(word[:4]) for line in lines for word in line
                       if needle.casefold() in word.text.casefold()]
            if matches:
                return matches[-1] if last else matches[0]
    return None


def wait_ui(window, identity, description=None, condition=lambda node: True):
    found = []
    def ready():
        node = UI_TRACES[window].node(identity)
        if node and visible_point(node) and condition(node):
            found[:] = [node]
            return True
        return False
    wait_for(ready, description or f"Painted UI node missing: {identity}")
    return found[0]


def click_ui(window, identity):
    node = wait_ui(window, identity)
    click(window, *visible_point(node))


def reveal_ui(window, predicate, description, scroll="transcript"):
    """Bounded scroll on viewport margin, outside nested scrollable details.

    Paint clips decide visibility. No guessed row height, badge count, or OCR.
    """
    trace = UI_TRACES[window]
    for _ in range(20):
        node = trace.matching(predicate)
        if node and visible_point(node):
            return node
        viewport = wait_ui(window, scroll)
        x, y, width, height = viewport["visible"]
        direction = 4 if node and node["bounds"][1] < y else 5
        generation = trace.generation
        wheel_at(window, round(x + 8), round(y + height / 2), direction, 3)
        expose_window(window)
        wait_for(lambda: (trace.poll() is not None and trace.generation > generation),
                 f"No paint after scrolling {scroll}")
    raise AssertionError(f"{description}\n{trace.describe()}")


def set_ui_expanded(window, identity, expanded):
    node = reveal_ui(window, lambda node: node["id"] == identity, f"Missing control: {identity}")
    if node["data"]["expanded"] != expanded:
        click(window, *visible_point(node))
    wait_for(lambda: (node := UI_TRACES[window].node(identity))
             and node["data"]["expanded"] == expanded, f"{identity}: expanded != {expanded}")


def open_tool_activity(window, call_id):
    node = reveal_ui(window, lambda node: node["id"].startswith("activity-")
                     and call_id in node["data"]["tools"], f"Activity missing: {call_id}")
    if node["data"]["expanded"]:
        # Running groups are open by default, not by user override. Completion
        # collapses them again, taking the finished card with it; wait for that
        # close so the reopen below records an explicit "stay open".
        wait_for(lambda: (current := UI_TRACES[window].node(node["id"]))
                 and not current["data"]["expanded"],
                 f"Activity did not finish: {call_id}")
    set_ui_expanded(window, node["id"], True)


def save_ui_trace(window, prefix):
    trace = UI_TRACES.get(window)
    if trace and trace.path.exists():
        path = tempfile.mktemp(prefix=prefix + "-ui-", suffix=".jsonl")
        Path(path).write_bytes(trace.path.read_bytes())
        print(f"UI paint trace: {path}")


def copy_ui_text(window, text):
    node = reveal_ui(window, lambda node: node["id"].startswith("selection-")
                     and node["data"].get("text") == text, "Selectable text missing")
    click(window, *visible_point(node))
    chord("Control_L", "a")
    chord("Control_L", "c")
    copied = clipboard_text()
    assert copied == text, repr(copied)
    return copied


def clipboard_text():
    # A copy action owns the X selection asynchronously; reading it in the same
    # tick can race the event loop. Retry the read, never the expected value.
    end = time.monotonic() + 5
    while time.monotonic() < end:
        probe = subprocess.run(["xclip", "-selection", "clipboard", "-out"],
                               capture_output=True, timeout=3)
        if probe.returncode == 0:
            return probe.stdout.decode()
        time.sleep(0.1)
    raise AssertionError("clipboard has no owner")


def drag(window, start, end):
    root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
    x11.XTranslateCoordinates(display, window, root_window, 0, 0,
                              ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
    def move(position):
        xtst.XTestFakeMotionEvent(display, -1, root_x.value + position[0], root_y.value + position[1], 0)
        x11.XFlush(display)
    move(start)
    xtst.XTestFakeButtonEvent(display, 1, True, 0)
    x11.XFlush(display)
    time.sleep(0.1)
    for step in range(1, 11):
        move(tuple(round(a + (b - a) * step / 10) for a, b in zip(start, end)))
        time.sleep(0.03)
    xtst.XTestFakeButtonEvent(display, 1, False, 0)
    x11.XFlush(display)
    time.sleep(0.2)


def wheel_at(window, x, y, button, count):
    """Scroll without activating whatever happens to be under the pointer."""
    root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
    x11.XTranslateCoordinates(display, window, root_window, x, y,
                              ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
    xtst.XTestFakeMotionEvent(display, -1, root_x.value, root_y.value, 0)
    for _ in range(count):
        xtst.XTestFakeButtonEvent(display, button, True, 0)
        xtst.XTestFakeButtonEvent(display, button, False, 0)
    x11.XFlush(display)


def scroll_up(window):
    click(window, 700, 300)
    for _ in range(8):
        xtst.XTestFakeButtonEvent(display, 4, True, 0)
        xtst.XTestFakeButtonEvent(display, 4, False, 0)
    x11.XFlush(display)
    time.sleep(0.3)


def click_nav_label(window, label, close=False, keyboard_fallback=True):
    if window in UI_TRACES:
        def matches(node):
            return node["id"].startswith("nav-") and label.casefold() in node["data"].get("title", "").casefold()
        row = reveal_ui(window, matches, f"Navigation label missing: {label}", scroll="nav-list")
        if close:
            click_ui(window, row["data"]["actions"])
        else:
            click(window, *visible_point(row))
            wait_for(lambda: UI_TRACES[window].matching(lambda node: matches(node) and node["data"]["active"]),
                     f"Navigation click did not activate {label}")
        return
    # Other smokes without tracing still use bounded OCR + keyboard fallback.
    # Busy indicators change row heights; saved sessions have no fixed y.
    def match_text(text):
        return "".join(char for char in text.lower() if char.isalnum())

    def find_row():
        # The narrow, tall rail makes tesseract's PSM 6 put every row in the
        # same logical line. Cluster words by their visual center instead of
        # trusting block/paragraph/line numbers from the TSV.
        with tempfile.NamedTemporaryFile(suffix=".png") as image:
            subprocess.run(["import", "-window", str(window), "-crop", "264x10000+0+0", "-resize", "200%", image.name], check=True)
            for psm in ("6", "11"):
                rows = subprocess.check_output(
                    ["tesseract", image.name, "stdout", "--psm", psm, "tsv"],
                    stderr=subprocess.DEVNULL).decode().splitlines()[1:]
                words = []
                for row in rows:
                    fields = row.split("\t")
                    if len(fields) != 12 or not fields[11].strip():
                        continue
                    if int(fields[6]) >= 528 or int(fields[9]) > 70:
                        continue  # outside the rail or a tall scrollbar glyph
                    words.append(fields)
                visual = []
                for fields in sorted(words, key=lambda f: (int(f[7]) + int(f[9]) // 2, int(f[6]))):
                    center = int(fields[7]) + int(fields[9]) // 2
                    for line in visual:
                        if abs(center - line["center"]) <= 16:
                            line["words"].append(fields)
                            line["center"] = (line["center"] + center) // 2
                            break
                    else:
                        visual.append({"center": center, "words": [fields]})
                for line in visual:
                    line_words = sorted(line["words"], key=lambda f: int(f[6]))
                    text = " ".join(word[11] for word in line_words)
                    if match_text(label) in match_text(text):
                        return line["center"] // 2
        return None

    y = None
    # A session row can appear a frame after its title; retry the same OCR
    # pass before deciding the row is absent.
    for _ in range(4):
        y = find_row()
        if y is not None:
            break
        time.sleep(0.25)
    if y is None:
        # Running rows grow activity lines; in a full list the requested
        # session can sit below the viewport. Start at the top and scan down,
        # so a row above the current scroll position is not skipped.
        for _ in range(12):
            wheel_at(window, 90, 420, 4, 4)
            time.sleep(0.15)
        y = find_row()
        for _ in range(20):
            if y is not None:
                break
            wheel_at(window, 90, 420, 5, 4)
            time.sleep(0.15)
            y = find_row()
    if y is None and not close and keyboard_fallback:
        print(f"Navigation OCR missed {label!r}; using keyboard fallback", flush=True)
        # Last resort: use the navigation search that the user has. This is
        # still a real selection; the caller's following state assertions
        # (active session, prompt, dialog response) decide whether it worked.
        chord("Control_L", "k")
        chord("Control_L", "a")
        press("BackSpace")
        type_text(label)
        time.sleep(0.3)
        press("Down")
        press("Return")
        # Selecting through search keeps the query in the field; clear it the
        # same way the keyboard session helper does.
        chord("Control_L", "k")
        chord("Control_L", "a")
        press("BackSpace")
        wait_for(lambda: find_word(window, "Filtrar") is not None,
                 f"Keyboard fallback did not clear search for {label}")
        # Search owns the keyboard until the user returns to the conversation;
        # the normal row click does this through Workspace::select.
        chord("Control_L", "l")
        return
    if y is None:
        raise AssertionError(f"Navigation label not found: {label}")
    click(window, 230 if close else 90, y)


def click_beta_session(window, close=False, keyboard_fallback=True):
    click_nav_label(window, "beta saved", close, keyboard_fallback)


def park_mouse(window):
    """Normalize focus/hover so pixel checks compare scroll, not focus styles."""
    chord("Control_L", "l")
    root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
    x11.XTranslateCoordinates(display, window, root_window, 5, 5,
                              ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
    xtst.XTestFakeMotionEvent(display, -1, root_x.value, root_y.value, 0)
    x11.XFlush(display)


def transcript_pixels(window, crop="600x400+280+100"):
    return subprocess.check_output(
        ["import", "-window", str(window), "-crop", crop, "-depth", "8", "rgb:-"])


def stable_transcript_pixels(window, crop="600x400+280+100"):
    previous = [transcript_pixels(window, crop)]
    def stable():
        current = transcript_pixels(window, crop)
        same = current == previous[0]
        previous[0] = current
        return same
    wait_for(stable, "Transcript layout did not settle", timeout=3)
    return previous[0]


def request_window_close(window):
    class ClientMessage(ctypes.Structure):
        _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong),
                    ("send_event", ctypes.c_int), ("display", ctypes.c_void_p),
                    ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
                    ("format", ctypes.c_int), ("data", ctypes.c_long * 5)]
    event = ctypes.create_string_buffer(192)
    message = ctypes.cast(event, ctypes.POINTER(ClientMessage)).contents
    message.type = 33
    message.display = display
    message.window = window
    message.message_type = x11.XInternAtom(display, b"WM_PROTOCOLS", False)
    message.format = 32
    message.data[0] = x11.XInternAtom(display, b"WM_DELETE_WINDOW", False)
    x11.XSendEvent(display, window, False, 0, event)
    x11.XFlush(display)
    time.sleep(0.3)


def chord(*names):
    for name in names:
        key(name, True)
    for name in reversed(names):
        key(name, False)
    time.sleep(0.2)


with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:
    temp = Path(temp)
    projects = [temp / "alpha", temp / "beta"]
    agent = temp / "agent"
    (agent / "sessions" / "test").mkdir(parents=True)
    for project in projects:
        project.mkdir()
        session = agent / "sessions" / "test" / f"{project.name}.jsonl"
        session.write_text(json.dumps({"type": "session", "cwd": str(project), "id": project.name}) +
                           "\n" + json.dumps({"type": "session_info", "name": f"{project.name} saved"}) + "\n" +
                           json.dumps({"type": "message", "message": {"role": "assistant", "content": [
                               {"type": "text", "text": f"ARCHIVESEARCH {project.name} CONTENTONLY"}]}}) + "\n")
    subprocess.run(["git", "-C", str(projects[0]), "init", "-b", "gitcontext-main"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    (projects[0] / "tracked.txt").write_text("one\ntwo\nthree\n")
    subprocess.run(["git", "-C", str(projects[0]), "add", "tracked.txt"], check=True)
    subprocess.run(["git", "-C", str(projects[0]), "-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.invalid",
                    "-c", "commit.gpgsign=false", "commit", "-m", "fixture"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(agent),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11",
               DISH_TEST_UI_LOG=str(temp / "ui.jsonl"))
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    # A aparência segue o desktop por padrão; estes testes verificam o visual
    # escuro, então a preferência fica fixada como numa instalação existente.
    (temp / "config/dish").mkdir(parents=True, exist_ok=True)
    (temp / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))

    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(projects[0])], env=env, stdout=log, stderr=log)

    def events():
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines() if line]

    def commands(kind):
        return [item for item in events() if item.get("command", {}).get("type") == kind]

    def active_project():
        # The workspace persists the active session on the next render; this is
        # the real selected state, not an OCR guess about the highlight.
        try:
            return json.loads((temp / "config/dish/state.json").read_text()).get("last_project")
        except (OSError, json.JSONDecodeError):
            return None

    def activate_beta():
        try:
            # While alpha's hidden dialog is open, typing in the nav search
            # would reach the modal editor. Disable that fallback here and use
            # conversation order instead: the only open sessions are alpha
            # and beta, so Ctrl+Tab moves to beta without typing anywhere.
            click_beta_session(window, keyboard_fallback=False)
        except AssertionError:
            chord("Control_L", "Tab")
            return active_project() == str(projects[1])
        return active_project() == str(projects[1])

    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        UI_TRACES[window] = UiTrace(temp / "ui.jsonl")
        prepare_window(window)
        wait_ui(window, "frame")
        wait_for(lambda: commands("get_messages"), "Initial Pi did not initialize")
        time.sleep(1)
        x11.XSetInputFocus(display, window, 1, 0)
        expose_window(window)
        # Native title/get_messages can precede layout. Target actual painted
        # input, then confirm focus; OCR is not a focus/readiness signal.
        wait_ui(window, "composer-input", "Composer did not render")
        wait_for(lambda: find_word(window, "gitcontext-main"), "Git branch footer did not render")
        click_ui(window, "composer-input")
        wait_ui(window, "composer-input", condition=lambda node: node["data"]["focused"])
        # Content search reaches archived message bodies, not just titles,
        # and stays scoped to the current project's sessions.
        type_text("draft-")
        chord("Control_L", "Shift_L", "f")
        type_text("ARCHIVESEARCH")
        search = wait_ui(window, "content-search", "Archived content search did not render",
                         lambda node: not node["data"]["loading"] and any(
                             "CONTENTONLY" in hit["snippet"] for hit in node["data"]["hits"]))
        assert all("alpha" in hit["title"] for hit in search["data"]["hits"]), search
        assert all("beta" not in hit["title"] for hit in search["data"]["hits"]), search
        search_screenshot = f"/tmp/dish-search-{next(tempfile._get_candidate_names())}.png"
        subprocess.run(["import", "-window", str(window), search_screenshot], check=True)
        press("Escape")
        wait_for(lambda: "content-search" not in UI_TRACES[window].poll(), "Escape did not close content search")
        chord("Control_L", "k")
        type_text("saved")
        press("Escape")
        type_text("returned")
        # Esc must focus the prompt, not leave text in the sidebar filter.
        wait_ui(window, "frame", "Sidebar Escape did not focus prompt",
                lambda node: node["data"]["draft"] == "draft-returned")
        def copied_draft():
            # Key events can be delivered before the escape/focus frame lands;
            # repeat the copy and judge the clipboard content, never skip it.
            chord("Control_L", "a")
            chord("Control_L", "c")
            try:
                return clipboard_text() == "draft-returned"
            except AssertionError:
                return False

        wait_for(copied_draft, "Search lost the prompt draft or did not restore focus")
        assert not commands("prompt"), "Search shortcuts accidentally submitted a prompt"
        press("BackSpace")
        chord("Control_L", "k")
        chord("Control_L", "a")
        press("BackSpace")
        press("Tab")
        chord("Shift_L", "Tab")
        press("Escape")
        (projects[0] / "tracked.txt").write_text("one\nreplacement\n")
        (projects[0] / "synthetic-change.txt").write_text("temporary Git fixture\n")
        type_text("long-a")
        press("Return")
        wait_for(lambda: commands("prompt"), "First prompt not sent")
        wait_for(lambda: find_word(window, "altera"), "Git footer did not refresh after the turn started")
        git_screenshot = f"/tmp/dish-git-context-{next(tempfile._get_candidate_names())}.png"
        subprocess.run(["import", "-window", str(window), git_screenshot], check=True)
        print("Git context screenshot:", git_screenshot)
        chord("Control_L", "f")
        type_text("long-a")
        wait_ui(window, "content-search", "Current-session content search did not render",
                lambda node: not node["data"]["loading"] and bool(node["data"]["hits"]))
        press("Tab")
        chord("Control_L", "a")
        type_text("tab-navigation-query")
        wait_ui(window, "content-search", "Query field did not retain focus after Tab",
                lambda node: node["data"]["query"] == "tab-navigation-query")
        assert UI_TRACES[window].node("frame")["data"]["draft"] == "", "Tab moved focus to the prompt"
        chord("Control_L", "a")
        type_text("long-a")
        chord("Shift_L", "Tab")
        press("Down")
        press("Up")
        assert UI_TRACES[window].node("content-search"), "Navigating results closed the search"
        wait_ui(window, "content-search", "Search results did not settle",
                lambda node: not node["data"]["loading"] and bool(node["data"]["hits"]))
        press("Return")
        wait_for(lambda: "content-search" not in UI_TRACES[window].poll(),
                 "Enter did not open the selected result and close search")
        assert len(commands("prompt")) == 1, "Navigating matches submitted another prompt"
        assert not commands("abort"), "Leaving search canceled the running agent"
        # Exercise the cleared composer while the first session is still running.
        click(window, 550, 780)
        # Open beta's saved session from its project group while alpha runs.
        click_beta_session(window)
        # A run can finish between the OCR snapshot and the click, changing row
        # heights. Re-locate with bounded retries instead of using stale geometry.
        wait_for(lambda: len(commands("get_messages")) == 2 or
                 (click_beta_session(window) or len(commands("get_messages")) == 2),
                 "Saved session did not open")
        wait_for(lambda: find_word(window, "reposit"), "Switching sessions retained the previous Git context")
        # Wheel events over the search header must not scroll the transcript
        # behind it, even though the header itself has no scrollable content.
        chord("Control_L", "f")
        type_text("Saved")
        wait_ui(window, "content-search", "Saved-message search did not render",
                lambda node: not node["data"]["loading"] and node["data"]["query"] == "saved")
        # O OCR pode localizar o cabeçalho alguns pixels fora em fontes diferentes;
        # repete a checagem antes de acusar vazamento do scroll.
        for attempt in range(3):
            panel = wait_ui(window, "content-search")
            x, y, _, _ = panel["bounds"]
            click(window, round(x + 12), round(y + 20))
            # Exclude overlay using its painted left edge, not an OCR heading.
            background_crop = f"{max(64, round(x) - 312)}x260+300+130"
            before_search_wheel = stable_transcript_pixels(window, background_crop)
            for _ in range(6):
                xtst.XTestFakeButtonEvent(display, 4, True, 0)
                xtst.XTestFakeButtonEvent(display, 4, False, 0)
            x11.XFlush(display)
            if stable_transcript_pixels(window, background_crop) == before_search_wheel:
                break
        else:
            raise AssertionError("Search wheel leaked to the underlying transcript")
        press("Escape")
        click(window, 550, 780)
        type_text("long-b")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 2, "Second prompt not sent")
        type_text("draft-b")
        # Return to alpha (the initial, unsaved conversation).
        click_nav_label(window, "long-a")
        type_text("draft-a")
        # Return to beta, whose draft must survive.
        click_beta_session(window)
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 3, "Draft prompt not sent")
        prompts = commands("prompt")
        assert prompts[0]["command"]["message"] == "long-a"
        assert prompts[1]["command"]["message"] == "long-b"
        assert prompts[2]["command"]["message"] == "draft-b", prompts
        assert prompts[0]["pid"] != prompts[1]["pid"]
        assert prompts[1]["pid"] == prompts[2]["pid"]
        assert not commands("abort"), "Navigation aborted a task"
        assert len({item["pid"] for item in commands("get_messages")}) == 2
        wait_for(lambda: len([item for item in events() if item.get("record", {}).get("type") == "agent_end"]) >= 3,
                 "Background tasks did not finish")
        time.sleep(0.3)
        scroll_up(window)
        park_mouse(window)
        scrolled = stable_transcript_pixels(window)
        click_nav_label(window, "long-a")
        click_beta_session(window)
        park_mouse(window)
        wait_for(lambda: transcript_pixels(window) == scrolled,
                 "Transcript scroll position was lost", timeout=3)
        click_nav_label(window, "long-a")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 4, "Alpha draft was lost")
        assert commands("prompt")[3]["command"]["message"] == "draft-a"
        # A hidden extension dialog must neither take beta's input nor stop navigation.
        type_text("dialog")
        press("Return")
        # A row can move while alpha's activity line grows; re-locate and click
        # until the workspace actually reports beta as the active session.
        wait_for(activate_beta, "Saved session did not become active", timeout=60)
        wait_for(lambda: any(item.get("record", {}).get("type") == "extension_ui_request" for item in events()),
                 "Background extension dialog did not arrive")
        time.sleep(0.2)
        # Alpha has just changed to max; beta's visible accent must remain low.
        accent_pixels = visible_accent_pixels(window)
        assert contains_rendered_color(accent_pixels, (84, 137, 164)), "Background effort changed the visible accent"
        type_text("still-beta")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 6, "Hidden dialog stole keyboard focus", timeout=30)
        assert commands("prompt")[-1]["pid"] == prompts[1]["pid"]
        click_nav_label(window, "dialog")
        # The click (or its keyboard fallback) must really have selected the
        # alpha conversation before the answer is typed.
        wait_for(lambda: active_project() == str(projects[0]),
                 "Dialog session did not become active")
        type_text("answer")
        press("Return")
        wait_for(lambda: commands("extension_ui_response"), "Pending dialog could not be answered")
        response = commands("extension_ui_response")[-1]["command"]
        assert response["value"] == "answer", response
        click_beta_session(window)
        type_text("long-close")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 7, "Close test prompt not sent")
        click_beta_session(window, close=True)
        # The menu opens on "Fixar"; move to "Fechar sessão" before Enter.
        press("Down")
        press("Return")
        wait_ui(window, "frame", "Close confirmation did not open",
                lambda node: node["data"]["confirming_close"])
        assert len(commands("prompt")) == 7, "Close confirmation did not isolate keyboard input"
        press("Escape")
        wait_ui(window, "frame", "Escape did not cancel the close",
                lambda node: not node["data"]["confirming_close"] and not node["data"]["session_menu"])
        assert not commands("abort"), "Canceling close interrupted a task"
        # Creating a session while beta runs must allocate a new process, not new_session.
        chord("Control_L", "l")
        chord("Control_L", "n")
        wait_for(lambda: len(commands("get_messages")) == 3, "Ctrl+N did not create an independent session")
        type_text("/new")
        press("Return")
        wait_for(lambda: len(commands("get_messages")) == 4, "/new did not create an independent session")
        assert not commands("new_session"), "A new session replaced an existing process"
        # New-session button follows the same workspace path.
        # Locate the named action: navigation width and header layout may change.
        click_ui(window, "nav-new-session")
        wait_for(lambda: len(commands("get_messages")) == 5, "Details button did not create an independent session")
        # Start an active task, then exercise the OS window-close path.
        type_text("long-window-close")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 8, "Window-close test task did not start")
        request_window_close(window)
        assert process.poll() is None, "Window closed without confirming active tasks"
        type_text("must-not-send")
        press("Return")
        assert len(commands("prompt")) == 8, "Window-close confirmation leaked keyboard input"
        press("Escape")
        assert find_window(), "Canceling close removed the window"
        wait_for(lambda: any(item.get("record", {}).get("type") == "agent_end"
                             and item.get("pid") == commands("prompt")[-1]["pid"] for item in events()),
                 "Last task did not finish")
        time.sleep(0.3)
        type_text("table")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 9, "Table prompt not sent")
        time.sleep(1.4)
        table_screenshot = tempfile.mktemp(prefix="dish-table-", suffix=".png")
        subprocess.run(["import", "-window", str(window), table_screenshot], check=True)
        print(f"Table screenshot: {table_screenshot}")

        # Native image paste, with no typed prompt, must send actual image data.
        image_file = temp / "clipboard.png"
        subprocess.run(["convert", "-size", "24x24", "xc:red", str(image_file)], check=True)
        def clipboard(data, mime):
            holder = subprocess.Popen(["xclip", "-selection", "clipboard", "-in", "-target", mime, "-quiet"],
                                      stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            holder.stdin.write(data)
            holder.stdin.close()
            # O dono do clipboard precisa assumir a seleção antes do Ctrl+V;
            # um sleep fixo perdia a corrida em CI carregada.
            end = time.monotonic() + 5
            while time.monotonic() < end:
                probe = subprocess.run(["xclip", "-selection", "clipboard", "-out", "-target", mime],
                                       capture_output=True, timeout=5)
                if probe.stdout:
                    break
                time.sleep(0.1)
            return holder

        holder = clipboard(image_file.read_bytes(), "image/png")
        try:
            # Ctrl+V e Return são repetidos juntos: uma leitura vazia do
            # clipboard descarta a colagem, e Return durante o carregamento é
            # ignorado de propósito.
            deadline = time.monotonic() + 30
            while len(commands("prompt")) == 9 and time.monotonic() < deadline:
                chord("Control_L", "v")
                time.sleep(1.0)
                press("Return")
                time.sleep(1.0)
            wait_for(lambda: len(commands("prompt")) == 10, "Pasted image not sent")
            import base64
            image_payload = commands("prompt")[-1]["command"]["images"][0]
            assert image_payload["mimeType"] == "image/png"
            assert base64.b64decode(image_payload["data"]) == image_file.read_bytes()
        finally:
            holder.terminate()
            holder.wait(timeout=3)
        time.sleep(1.3)

        # File managers often offer only URI-list, which GPUI does not request.
        holder = clipboard((image_file.as_uri() + "\r\n").encode(), "text/uri-list")
        try:
            deadline = time.monotonic() + 30
            while len(commands("prompt")) == 10 and time.monotonic() < deadline:
                chord("Control_L", "v")
                time.sleep(1.0)
                press("Return")
                time.sleep(1.0)
            wait_for(lambda: len(commands("prompt")) == 11, "Copied image file not sent")
            assert commands("prompt")[-1]["command"]["images"][0]["data"] == image_payload["data"]
        finally:
            holder.terminate()
            holder.wait(timeout=3)
        time.sleep(1.3)

        type_text("long-scroll")
        press("Return")
        wait_for(lambda: any("Parágrafo de teste" in item.get("record", {}).get("assistantMessageEvent", {}).get("delta", "")
                             for item in events()), "Large streaming message not received")
        scroll_up(window)
        park_mouse(window)
        before = stable_transcript_pixels(window)
        time.sleep(0.65)
        assert transcript_pixels(window) == before, "Streaming pulled the reader back to the bottom"
        # Direct selection on formatted text, including Unicode and code.
        click_ui(window, "composer-input")
        initializations = len(commands("get_messages"))
        chord("Control_L", "n")
        wait_for(lambda: len(commands("get_messages")) > initializations,
                 "Selection test conversation did not initialize")
        chord("Control_L", "l")
        previous_prompts = len(commands("prompt"))
        type_text("selection")
        press("Return")
        wait_for(lambda: len(commands("prompt")) > previous_prompts, "Selection prompt not sent")
        wait_for(lambda: any(item.get("record", {}).get("type") == "message_end"
                             and "Final da resposta." in str(item["record"]) for item in events()),
                 "Selection response did not finish")
        selection = wait_for(lambda: UI_TRACES[window].matching(lambda node:
            node["id"].startswith("selection-reply-text-")
            and node["data"].get("text", "").startswith("Primeiro parágrafo")),
            "Formatted selection response not painted")
        selection_crop = tuple(round(value) for value in selection["visible"])
        first = wait_for(lambda: find_word(window, "Primeiro", crop=selection_crop),
                         "Formatted selection response not rendered")
        click(window, first[0] + first[2] // 2, first[1] + first[3] // 2)
        click(window, first[0] + first[2] // 2, first[1] + first[3] // 2)
        chord("Control_L", "c")
        assert clipboard_text() == "Primeiro", "Double click did not copy a rendered word"
        chord("Control_L", "a")
        chord("Control_L", "c")
        expected_selection = ("Primeiro parágrafo com ação e café.\nSegundo parágrafo selecionável.\n"
                              "printf 'ação\\n'\necho café\nFinal da resposta.")
        assert clipboard_text() == expected_selection, repr(clipboard_text())
        # The reply icon copies the complete Markdown, whereas direct selection
        # copies rendered text. Both actions remain usable without the mouse.
        chord("Shift_L", "Tab")
        press("Return")
        time.sleep(0.2)
        expected_markdown = ("Primeiro **parágrafo** com ação e café.\n\n"
                             "Segundo parágrafo selecionável.\n\n"
                             "```sh\nprintf 'ação\\n'\necho café\n```\n\nFinal da resposta.")
        assert clipboard_text() == expected_markdown, repr(clipboard_text())
        click(window, first[0] + first[2] // 2, first[1] + first[3] // 2)
        # Tab from the selection container reaches the code copy icon; Enter
        # copies raw code (not Markdown fences), without sending a new prompt.
        press("Tab")
        press("Return")
        time.sleep(0.2)
        assert clipboard_text() == "printf 'ação\\n'\necho café", repr(clipboard_text())
        assert len(commands("prompt")) == previous_prompts + 1, "Copy activated the composer"
        last = wait_for(lambda: find_word(window, "café", last=True, crop=selection_crop),
                        "Final code word missing from selection")
        drag(window, (first[0] + 1, first[1] + first[3] // 2),
             (last[0] + last[2] - 1, last[1] + last[3] // 2))
        chord("Control_L", "c")
        selected = clipboard_text()
        assert "parágrafo com ação e café.\nSegundo parágrafo selecionável.\nprintf" in selected, repr(selected)
        assert "Final da resposta" not in selected, repr(selected)
        drag(window, (last[0] + last[2] - 1, last[1] + last[3] // 2),
             (first[0] + 1, first[1] + first[3] // 2))
        chord("Control_L", "c")
        assert clipboard_text() == selected, "Reverse drag changed the selected text"
        press("Escape")
        # Clearing selection must not destroy the existing clipboard contents.
        chord("Control_L", "c")
        assert clipboard_text() == selected
        # Return to the composer and paste: selecting transcript text must not
        # have mutated the response or turned it into an editable field.
        chord("Control_L", "l")
        chord("Control_L", "v")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == previous_prompts + 2, "Selected text was not pasted")
        assert commands("prompt")[-1]["command"]["message"] == selected
        screenshot = tempfile.mktemp(prefix="dish-desktop-", suffix=".png")
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        assert process.poll() is None, (temp / "dish.log").read_text()
        print(f"Desktop smoke passed; screenshot: {screenshot}")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-desktop-failure-", suffix=".png")
        subprocess.run(["import", "-window", "root", screenshot], check=False)
        print(f"Desktop failure screenshot: {screenshot}")
        save_ui_trace(locals().get("window"), "dish-desktop")
        print((temp / "dish.log").read_text())
        raise
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        log.close()
        for pid in {item["pid"] for item in events()}:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass
        if process.returncode not in (0, -15):
            print((temp / "dish.log").read_text())

# Metadata: a fresh process with synthetic historical messages and fixed TZ.
with tempfile.TemporaryDirectory(prefix="dish-metadata-") as directory:
    directory = Path(directory)
    wrapper = directory / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    metadata_log = directory / "events.jsonl"
    metadata_env = dict(os.environ, DISH_PI_BIN=str(wrapper),
        PI_CODING_AGENT_DIR=str(directory / "agent"),
        XDG_CONFIG_HOME=str(directory / "config"), XDG_DATA_HOME=str(directory / "data"),
        DISH_FAKE_PI_LOG=str(metadata_log), TZ="UTC", XDG_SESSION_TYPE="x11",
        DISH_TEST_UI_LOG=str(directory / "ui.jsonl"),
        DISH_FAKE_METADATA_HISTORY=str(ROOT / "tests/fixtures/metadata_history.json"))
    metadata_env.pop("WAYLAND_DISPLAY", None)
    metadata_env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    def metadata_events():
        return [json.loads(line) for line in metadata_log.read_text().splitlines()] if metadata_log.exists() else []
    def screen_text(window):
        with tempfile.NamedTemporaryFile(suffix=".png") as image:
            subprocess.run(["import", "-window", str(window), image.name], check=True)
            return subprocess.check_output(["tesseract", image.name, "stdout", "--psm", "11"],
                                           stderr=subprocess.DEVNULL).decode()
    with open(directory / "dish.log", "w") as log:
        # Mesmo motivo do teste principal: o visual verificado é o escuro.
        (directory / "config/dish").mkdir(parents=True, exist_ok=True)
        (directory / "config/dish/state.json").write_text(json.dumps({"appearance": "dark"}))
        process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(directory)],
                                   env=metadata_env, stdout=log, stderr=log)
        try:
            wait_for(lambda: find_window(), "Metadata window not created")
            window = find_window()
            UI_TRACES[window] = UiTrace(directory / "ui.jsonl")
            prepare_window(window)
            wait_ui(window, "frame")
            wait_for(lambda: any(item.get("command", {}).get("type") == "get_messages"
                                 for item in metadata_events()), "Metadata Pi did not initialize")
            x11.XSetInputFocus(display, window, 1, 0)
            expose_window(window)
            wait_for(lambda: find_word(window, "Historico"), "Synthetic history did not render")
            initial = screen_text(window)
            assert "03/12" in initial, initial
            assert "interromp" in initial.lower(), initial
            import datetime
            assert datetime.datetime.now(
                datetime.timezone.utc).strftime("%d/%m/%Y") not in initial, \
                "History received fabricated current timestamps"
            # Stable call identity + actual painted bounds, not "last comando"
            # or "second fim". Actions still go through real XTest and clipboard.
            history = json.loads((ROOT / "tests/fixtures/metadata_history.json").read_text())
            call = history[1]["content"][0]
            call_id = call["id"]
            expected_command = call["arguments"]["command"]
            open_tool_activity(window, call_id)
            set_ui_expanded(window, f"tool-{call_id}", True)
            copy_ui_text(window, expected_command)
            set_ui_expanded(window, f"tool-{call_id}", False)
            # Tooltip is a visual assertion; keep OCR here, enlarged/multi-PSM.
            wait_for(lambda: find_word(window, "COMANDO_COMPLETO"), "Full command tooltip missing", timeout=4)
            park_mouse(window)
            badge_id = f"tool-metadata-{call_id}-badge"
            set_ui_expanded(window, badge_id, True)
            park_mouse(window)
            details = wait_ui(window, f"tool-metadata-{call_id}-details")
            historical_metadata = copy_ui_text(window, details["data"]["text"])
            assert "Resultado registrado pelo Pi" in historical_metadata, historical_metadata
            assert "03/12/2024 14:00:03.000 +00:00" in historical_metadata, historical_metadata
            assert "Duração" not in historical_metadata and "Fim observado" not in historical_metadata, historical_metadata
            # Pi commands/events prove protocol delivery, not the UI's expanded
            # state. Trace observes the UI; clipboard checks actual rendered text.
            chord("Control_L", "l")
            type_text("metadata-tool")
            press("Return")
            wait_for(lambda: any(item.get("record", {}).get("type") == "tool_execution_end" for item in metadata_events()), "Live tool did not finish")
            wait_for(lambda: any(item.get("record", {}).get("type") == "agent_end" for item in metadata_events()), "Live tool run did not finish")
            live_call = next(item["record"]["toolCallId"] for item in metadata_events()
                             if item.get("record", {}).get("type") == "tool_execution_end")
            # Real jump-to-end action avoids wheel events landing in a nested
            # metadata scroller. Any later reveal scrolls the viewport margin.
            if not UI_TRACES[window].node("transcript")["data"]["following_tail"]:
                click_ui(window, "transcript-jump-end")
            open_tool_activity(window, live_call)
            badge_id = f"tool-metadata-{live_call}-badge"
            badge = reveal_ui(window, lambda node: node["id"] == badge_id
                             and node["data"]["label"].startswith("fim ·")
                             and "Duração observada pelo Dish (relógio monotônico)" in node["data"]["details"],
                             "Live completion timestamp missing")
            expected_metadata = badge["data"]["details"]
            assert "Duração observada pelo Dish (relógio monotônico)" in expected_metadata, expected_metadata
            set_ui_expanded(window, badge_id, True)
            reveal_ui(window, lambda node: node["id"] == f"tool-metadata-{live_call}-details",
                      "Observed duration details missing")
            live_metadata = copy_ui_text(window, expected_metadata)
            assert "Início observado pelo Dish" in live_metadata, live_metadata
            assert "Fim observado pelo Dish" in live_metadata, live_metadata
            assert "Resultado registrado pelo Pi" in live_metadata, live_metadata
            assert "Duração observada pelo Dish (relógio monotônico)" in live_metadata, live_metadata
            assert process.poll() is None, (directory / "dish.log").read_text()
            print("Metadata smoke passed: history, cancellation, full commands, timestamps and duration")
        except Exception:
            screenshot = tempfile.mktemp(prefix="dish-metadata-failure-", suffix=".png")
            subprocess.run(["import", "-window", "root", screenshot], check=False)
            print(f"Metadata failure screenshot: {screenshot}")
            save_ui_trace(locals().get("window"), "dish-metadata")
            print((directory / "dish.log").read_text())
            raise
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            for pid in {item["pid"] for item in metadata_events()}:
                try:
                    os.kill(pid, 15)
                except ProcessLookupError:
                    pass
