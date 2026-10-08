"""Run with xvfb-run -a python3 tests/startup_smoke.py after cargo build --release.

Uses only fake Node/npm/Pi executables. No network or user configuration changes.
"""
from pathlib import Path

# Reuse XTest helpers without running the existing-session test scenario.
helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split(
    'with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

with tempfile.TemporaryDirectory(prefix="dish-startup-") as temporary:
    temp = Path(temporary)
    bin_dir = temp / "bin"
    bin_dir.mkdir()
    log_path = temp / "events.jsonl"
    install_log = temp / "install-called"
    terminal_log = temp / "terminal-called"

    def script(name, content):
        path = bin_dir / name
        path.write_text("#!/bin/sh\n" + content + "\n")
        path.chmod(0o700)
        return path

    script("node", "printf 'v22.19.0'")
    script("x-terminal-emulator", f'''printf '%s\\n' "$@" > "{terminal_log}"''')
    script("npm", f'''if [ "$1" = --version ]; then printf '10.0.0'; exit 0; fi
test "$1" = install && test "$3" = --ignore-scripts || exit 2
printf 'installed\\n' >> "{install_log}"
/bin/mkdir -p "$7/bin"
printf '#!/bin/sh\\nexec /usr/bin/python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\\n' > "$7/bin/pi"
/bin/chmod +x "$7/bin/pi"
''')
    env = dict(os.environ, PATH=str(bin_dir),
               XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"),
               PI_CODING_AGENT_DIR=str(temp / "agent"),
               DISH_FAKE_PI_LOG=str(log_path), XDG_SESSION_TYPE="x11",
               RUST_BACKTRACE="1")
    env.pop("DISH_PI_BIN", None)
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)

    def events():
        if not log_path.exists():
            return []
        return [json.loads(line) for line in log_path.read_text().splitlines() if line]

    def launch():
        log = open(temp / "dish.log", "w")
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(temp)], env=env,
            stdout=log, stderr=log)
        return process, log

    def click_consent(label):
        # Fonts and line wrapping vary between local desktops and clean CI.
        # Find the consent button's text instead of assuming its vertical position.
        def locate():
            subprocess.run(["import", "-window", str(window), screenshot], check=True)
            rows = subprocess.check_output(
                ["tesseract", screenshot, "stdout", "--psm", "6", "tsv"],
                stderr=subprocess.DEVNULL).decode().splitlines()[1:]
            lines = {}
            for row in rows:
                fields = row.split("\t")
                if len(fields) == 12 and fields[11].strip():
                    lines.setdefault(tuple(fields[1:5]), []).append(fields)
            for words in lines.values():
                if label.lower() in " ".join(word[11] for word in words).lower():
                    left = min(int(word[6]) for word in words)
                    right = max(int(word[6]) + int(word[8]) for word in words)
                    first = words[0]
                    click(window, (left + right) // 2, int(first[7]) + int(first[9]) // 2)
                    return True
            return False
        wait_for(locate, f"Consent button not found: {label}")

    process, log = launch()
    try:
        wait_for(lambda: find_window(), "Setup window not created")
        window = find_window()
        time.sleep(0.2)
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(2)
        assert process.poll() is None
        assert not install_log.exists(), "Installation ran without consent"
        assert not terminal_log.exists(), "Official installer ran without consent"
        screenshot = tempfile.mktemp(prefix="dish-setup-", suffix=".png")
        subprocess.run(["import", "-window", str(window), screenshot], check=True)
        print(f"Setup screenshot: {screenshot}", flush=True)
        # Official consent launches only a fake terminal, never a remote script.
        click_consent("Aceitar e abrir instalador oficial")
        wait_for(lambda: terminal_log.exists(), "Official consent did not open terminal")
        arguments = terminal_log.read_text().splitlines()
        assert arguments[:3] == ["-e", "/bin/sh", "-c"]
        assert "curl -fsSL https://pi.dev/install.sh | sh" in arguments[3]
        assert not install_log.exists(), "Official option invoked npm instead"
        process.terminate()
        process.wait(timeout=5)
        log.close()
        process, log = launch()
        time.sleep(3)
        window = find_window()
        x11.XSetInputFocus(display, window, 1, 0)
        x11.XFlush(display)
        time.sleep(2)
        # Locate the alternative npm consent button after the fresh launch.
        click_consent("aceitar e instalar via npm")
        wait_for(lambda: install_log.exists(), "Consent did not start fake installation")
        wait_for(lambda: any(e.get("command", {}).get("type") == "get_messages" for e in events()),
                 "Installed Pi did not initialize")
        preferences = temp / "config/dish/state.json"
        wait_for(lambda: preferences.exists(), "Selected executable was not saved")
        assert json.loads(preferences.read_text())["pi_executable"] == str(temp / "data/dish/pi/bin/pi")
        initial_count = sum(e.get("command", {}).get("type") == "get_messages" for e in events())
        process.terminate()
        process.wait(timeout=5)
        log.close()
        process, log = launch()
        wait_for(lambda: sum(e.get("command", {}).get("type") == "get_messages" for e in events()) > initial_count,
                 "Installed Pi was not discovered after restart")
        assert install_log.read_text().splitlines() == ["installed"], "Restart reinstalled Pi"
        print("Startup installation smoke passed")
    except Exception:
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
        for pid in {e["pid"] for e in events()}:
            try:
                os.kill(pid, 15)
            except ProcessLookupError:
                pass
