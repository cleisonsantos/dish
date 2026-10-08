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
x11.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong,
                          ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
                          ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
x11.XFetchName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_char_p)]
x11.XFree.argtypes = [ctypes.c_void_p]
x11.XTranslateCoordinates.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
                                     ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_int),
                                     ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong)]
x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
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
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError(description)


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


# Geometria da navegação (largura 264): cabeçalho de 52px, bloco de
# busca/filtros de ~84px, linha de projeto de ~26px e linha de sessão de ~45px.
# ALPHA é a conversa viva do primeiro projeto; BETA é localizada pelo texto.
ALPHA_ROW = (90, 193)
# O ponto de accent visível: o marcador de esforço no compositor.
ACCENT_PROBE = "220x80+300+740"
# Botão de nova sessão no cabeçalho da navegação.
NEW_SESSION = (209, 25)


def click(window, x, y):
    root_x, root_y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
    x11.XTranslateCoordinates(display, window, root_window, 0, 0,
                              ctypes.byref(root_x), ctypes.byref(root_y), ctypes.byref(child))
    xtst.XTestFakeMotionEvent(display, -1, root_x.value + x, root_y.value + y, 0)
    xtst.XTestFakeButtonEvent(display, 1, True, 0)
    xtst.XTestFakeButtonEvent(display, 1, False, 0)
    x11.XFlush(display)
    time.sleep(0.2)


def scroll_up(window):
    click(window, 700, 300)
    for _ in range(8):
        xtst.XTestFakeButtonEvent(display, 4, True, 0)
        xtst.XTestFakeButtonEvent(display, 4, False, 0)
    x11.XFlush(display)
    time.sleep(0.3)


def click_beta_session(window, close=False):
    # Busy indicators change row heights; saved sessions have no fixed y.
    with tempfile.NamedTemporaryFile(suffix=".png") as image:
        subprocess.run(["import", "-window", str(window), "-crop", "264x10000+0+0", image.name], check=True)
        rows = subprocess.check_output(
            ["tesseract", image.name, "stdout", "--psm", "6", "tsv"],
            stderr=subprocess.DEVNULL).decode().splitlines()[1:]
    lines = {}
    for row in rows:
        fields = row.split("\t")
        if len(fields) == 12 and fields[11].strip() and int(fields[6]) < 264:
            lines.setdefault(tuple(fields[1:5]), []).append(fields)
    for words in lines.values():
        if "beta saved" in " ".join(word[11] for word in words).lower():
            first = words[0]
            click(window, 230 if close else 90, int(first[7]) + int(first[9]) // 2)
            return
    raise AssertionError("Beta session row not found")


def park_mouse(window):
    """Tira o ponteiro de cima da conversa: hover não deve entrar na captura."""
    click(window, 5, 5)


def transcript_pixels(window):
    return subprocess.check_output(
        ["import", "-window", str(window), "-crop", "600x400+280+100", "-depth", "8", "rgb:-"])


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
                           "\n" + json.dumps({"type": "session_info", "name": f"{project.name} saved"}) + "\n")
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    log_path = temp / "events.jsonl"
    env = dict(os.environ, DISH_PI_BIN=str(wrapper), PI_CODING_AGENT_DIR=str(agent),
               XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    log = open(temp / "dish.log", "w")
    process = subprocess.Popen([os.environ.get("DISH_TEST_BIN", str(ROOT / "target/debug/dish")), str(projects[0])], env=env, stdout=log, stderr=log)

    def events():
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines() if line]

    def commands(kind):
        return [item for item in events() if item.get("command", {}).get("type") == kind]

    try:
        wait_for(lambda: find_window(), "Window not created")
        window = find_window()
        wait_for(lambda: commands("get_messages"), "Initial Pi did not initialize")
        time.sleep(1)
        x11.XSetInputFocus(display, window, 1, 0)
        # Clicking placeholder glyphs must not put the caret past an empty buffer.
        click(window, 550, 780)
        type_text("long-a")
        press("Return")
        wait_for(lambda: commands("prompt"), "First prompt not sent")
        # Exercise the cleared composer while the first session is still running.
        click(window, 550, 780)
        # Open beta's saved session from its project group while alpha runs.
        click_beta_session(window)
        wait_for(lambda: len(commands("get_messages")) == 2, "Saved session did not open")
        click(window, 550, 780)
        type_text("long-b")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 2, "Second prompt not sent")
        type_text("draft-b")
        # Return to alpha (the initial, unsaved conversation).
        click(window, *ALPHA_ROW)
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
        scrolled = transcript_pixels(window)
        click(window, *ALPHA_ROW)
        click_beta_session(window)
        park_mouse(window)
        assert transcript_pixels(window) == scrolled, "Transcript scroll position was lost"
        click(window, *ALPHA_ROW)
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 4, "Alpha draft was lost")
        assert commands("prompt")[3]["command"]["message"] == "draft-a"
        # A hidden extension dialog must neither take beta's input nor stop navigation.
        type_text("dialog")
        press("Return")
        click_beta_session(window)
        wait_for(lambda: any(item.get("record", {}).get("type") == "extension_ui_request" for item in events()),
                 "Background extension dialog did not arrive")
        time.sleep(0.2)
        # Alpha has just changed to max; beta's visible accent must remain low.
        accent_pixels = subprocess.check_output(
            ["import", "-window", str(window), "-crop", ACCENT_PROBE, "-depth", "8", "rgb:-"])
        assert bytes((84, 137, 164)) in accent_pixels, "Background effort changed the visible accent"
        type_text("still-beta")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 6, "Hidden dialog stole keyboard focus")
        assert commands("prompt")[-1]["pid"] == prompts[1]["pid"]
        click(window, *ALPHA_ROW)
        type_text("answer")
        press("Return")
        wait_for(lambda: commands("extension_ui_response"), "Pending dialog could not be answered")
        assert commands("extension_ui_response")[-1]["command"]["value"] == "answer"
        click_beta_session(window)
        type_text("long-close")
        press("Return")
        wait_for(lambda: len(commands("prompt")) == 7, "Close test prompt not sent")
        click_beta_session(window, close=True)
        press("Return")
        assert len(commands("prompt")) == 7, "Close confirmation did not isolate keyboard input"
        press("Escape")
        assert not commands("abort"), "Canceling close interrupted a task"
        # Creating a session while beta runs must allocate a new process, not new_session.
        chord("Control_L", "n")
        wait_for(lambda: len(commands("get_messages")) == 3, "Ctrl+N did not create an independent session")
        type_text("/new")
        press("Return")
        wait_for(lambda: len(commands("get_messages")) == 4, "/new did not create an independent session")
        assert not commands("new_session"), "A new session replaced an existing process"
        # New-session button follows the same workspace path.
        click(window, *NEW_SESSION)
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
            time.sleep(0.2)
            return holder

        holder = clipboard(image_file.read_bytes(), "image/png")
        try:
            chord("Control_L", "v")
            time.sleep(0.4)
            press("Return")
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
            chord("Control_L", "v")
            time.sleep(0.4)
            press("Return")
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
        before = transcript_pixels(window)
        time.sleep(0.65)
        assert transcript_pixels(window) == before, "Streaming pulled the reader back to the bottom"
        screenshot = tempfile.mktemp(prefix="dish-desktop-", suffix=".png")
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        assert process.poll() is None, (temp / "dish.log").read_text()
        print(f"Desktop smoke passed; screenshot: {screenshot}")
    except Exception:
        screenshot = tempfile.mktemp(prefix="dish-desktop-failure-", suffix=".png")
        subprocess.run(["import", "-window", "root", screenshot], check=False)
        print(f"Desktop failure screenshot: {screenshot}")
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
