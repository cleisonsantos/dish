"""Xvfb: malformed Pi output, stderr and the crash report must not leak payloads."""
from pathlib import Path
import os
import subprocess
import tempfile

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))

SECRET = "sk-live-SYNTHETIC-PRIVACY-0123456789"


def crash_phase():
    """Pânico sintético: o crash.log nasce redigido e dentro do orçamento."""
    binary = os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish"))
    with tempfile.TemporaryDirectory(prefix="dish-crash-") as temporary:
        temp = Path(temporary)
        home = temp / "home"
        home.mkdir()
        state = temp / "state"
        state.mkdir()
        agent = temp / "agent"
        agent.mkdir()
        env = dict(os.environ, HOME=str(home), XDG_STATE_HOME=str(state),
                   PI_CODING_AGENT_DIR=str(agent),
                   DISH_TEST_PANIC=f"token={SECRET} {SECRET} {home}/projeto "
                                    f"{agent}/sessions/x/alpha.jsonl")
        env.pop("DISH_LOG_FILE", None)
        result = subprocess.run([binary], env=env, capture_output=True,
                                encoding="utf-8", errors="replace", timeout=30)
        assert result.returncode != 0, "test panic exited successfully"
        crash_log = state / "dish" / "crash.log"
        assert crash_log.is_file(), "crash.log was not written"
        report = crash_log.read_text(encoding="utf-8", errors="replace")
        for marker in ("Dish crash report", "dish-test-panic", "local:", "backtrace:"):
            assert marker in report, f"crash.log lacks {marker!r}"
        assert SECRET not in report, "panic payload leaked into crash.log"
        assert str(home) not in report, "home path leaked into crash.log"
        assert "alpha.jsonl" not in report, "session path leaked into crash.log"
        assert crash_log.stat().st_size <= 65536, "crash.log exceeded its budget"
        assert SECRET not in result.stderr, "panic payload leaked into stderr"
    print("Crash report privacy smoke passed")


crash_phase()

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
               XDG_STATE_HOME=str(temp / "state"), XDG_SESSION_TYPE="x11")
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("PI_CODING_AGENT_SESSION_DIR", None)
    env.pop("DISH_TEST_PANIC", None)
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
        # O mesmo aviso também vai para o arquivo de diagnóstico (issue #147).
        dish_log = temp / "state" / "dish" / "dish.log"
        if dish_log.exists():
            logged = dish_log.read_text()
            assert SECRET not in logged, "Pi diagnostic payload leaked into dish.log"
            assert "token=" not in logged, "Pi diagnostic field leaked into dish.log"
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
