"""Xvfb: malformed Pi output and stderr must not leak payloads into logs."""
from pathlib import Path
import os
import subprocess
import tempfile

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

SECRET = "sk-live-SYNTHETIC-PRIVACY-0123456789"

with tempfile.TemporaryDirectory(prefix="dish-privacy-") as temporary:
    temp = Path(temporary)
    home = temp / "home"
    home.mkdir()
    agent = temp / "agent"
    sessions = agent / "sessions" / "test"
    sessions.mkdir(parents=True)
    project = temp / "alpha"
    project.mkdir()
    session = sessions / "alpha.jsonl"
    session.write_text(
        '{"type":"session","cwd":"' + str(project) + '","id":"alpha"}\n'
        '{"type":"session_info","name":"Privacy session"}\n'
    )
    wrapper = temp / "fake-pi"
    wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
    wrapper.chmod(0o700)
    (temp / "config/dish").mkdir(parents=True)
    log_path = temp / "dish.log"
    log = open(log_path, "w")
    env = dict(os.environ, HOME=str(home), DISH_PI_BIN=str(wrapper),
               PI_CODING_AGENT_DIR=str(agent), XDG_CONFIG_HOME=str(temp / "config"),
               XDG_DATA_HOME=str(temp / "data"), DISH_FAKE_PI_DIAGNOSTIC="1",
               XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    process = None
    try:
        process = subprocess.Popen(
            [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")),
             "--session", str(session), str(project)],
            env=env, stdout=log, stderr=log)
        wait_for(lambda: find_window(), "Window not created")
        wait_for(lambda: "content omitted" in log_path.read_text(),
                 "safe stderr diagnostic was not written")
        wait_for(lambda: "unparsable record" in log_path.read_text(),
                 "malformed record summary was not written")
        text = log_path.read_text()
        assert SECRET not in text, "Pi diagnostic payload leaked into logs"
        assert "token=" not in text, "Pi diagnostic field leaked into logs"
        print("Diagnostic privacy smoke passed")
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        log.close()
