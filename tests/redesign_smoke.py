"""GPUI workbench regression smoke: wide/narrow, motion and real RPC flow.

Run in Xvfb after cargo build --release. Synthetic Pi and temporary homes only.
"""
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split('with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))


def activate_word(window, word):
    wait_for(lambda: find_word(window, word), f"Control missing: {word}")
    box = find_word(window, word)
    click(window, box[0] + box[2] // 2, box[1] + box[3] // 2)


for width, height, reduced in [(1600, 960, False), (780, 760, True)]:
    with tempfile.TemporaryDirectory(prefix="dish-redesign-") as temporary:
        temp = Path(temporary)
        project = temp / "projeto-sintetico"
        project.mkdir()
        (temp / "home").mkdir()
        config = temp / "config" / "dish"
        config.mkdir(parents=True)
        preferences = config / "state.json"
        preferences.write_text(json.dumps({
            "appearance": "dark", "window_size": [width, height],
            "reduce_motion": reduced,
        }))
        wrapper = temp / "fake-pi"
        wrapper.write_text(f'#!/bin/sh\nexec python3 "{ROOT}/tests/fixtures/fake_pi.py" "$@"\n')
        wrapper.chmod(0o700)
        events_file = temp / "events.jsonl"
        env = dict(os.environ, HOME=str(temp / "home"), DISH_PI_BIN=str(wrapper),
                   PI_CODING_AGENT_DIR=str(temp / "agent"),
                   XDG_CONFIG_HOME=str(temp / "config"), XDG_DATA_HOME=str(temp / "data"),
                   DISH_FAKE_PI_LOG=str(events_file), XDG_SESSION_TYPE="x11")
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("PI_CODING_AGENT_SESSION_DIR", None)

        def events():
            records = []
            if events_file.exists():
                for line in events_file.read_text().splitlines():
                    try:
                        records.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass  # Last JSONL frame may still be in flight.
            return records

        def commands(kind):
            return [item["command"] for item in events()
                    if item.get("command", {}).get("type") == kind]

        def saved_motion():
            try:
                return json.loads(preferences.read_text()).get("reduce_motion")
            except (OSError, json.JSONDecodeError):
                return None

        def capture(label):
            path = tempfile.mktemp(prefix=f"dish-redesign-{width}-{label}-", suffix=".png")
            subprocess.run(["import", "-window", str(window) if window else "root", path], check=True)
            print(f"{label}: {path}")

        with open(temp / "dish.log", "w+") as log:
            process = subprocess.Popen(
                [os.environ.get("DISH_TEST_BIN", str(ROOT / "target/release/dish")), str(project)],
                env=env, stdout=log, stderr=log)
            window = None
            try:
                wait_for(lambda: find_window(), "Window not created")
                window = find_window()
                wait_for(lambda: commands("get_available_models"), "Pi did not initialize")
                prepare_window(window)
                wait_for(lambda: find_word(window, "Qual"), "Welcome did not render")
                wait_for(lambda: find_word(window, "Enviar"), "Send action missing")
                # The light enabled control is poorly OCR'd by some versions.
                # Anchor its label while idle; one-line drafts reserve the same height.
                send_box = find_word(window, "Enviar")
                capture("welcome")

                # Suggestions fill the editor; they must never submit by themselves.
                activate_word(window, "arquitetura")
                assert not commands("prompt"), "Suggestion submitted without confirmation"
                type_text(" agora")
                click(window, send_box[0] + send_box[2] // 2, send_box[1] + send_box[3] // 2)
                wait_for(lambda: commands("prompt"), "Suggestion did not reach Pi")
                assert commands("prompt")[-1]["message"] == "Explique a arquitetura deste projeto agora"
                wait_for(lambda: find_word(window, "Finished"), "Response did not render")
                capture("conversation")

                # Mouse activation must return typing focus to the editor.
                type_text("draft-")
                activate_word(window, "baixo")
                activate_word(window, "alto")
                wait_for(lambda: commands("set_thinking_level")
                         and commands("set_thinking_level")[-1].get("level") == "high",
                         "Effort choice did not reach Pi")
                type_text("effort-")

                # The trigger also acts as a keyboard-selectable combobox.
                activate_word(window, "alto")
                press("Down")
                press("Return")
                wait_for(lambda: commands("set_thinking_level")[-1].get("level") == "max",
                         "Keyboard effort did not reach Pi")
                type_text("keyboard-")

                press("F1")
                activate_word(window, "Apar")
                activate_word(window, "Reduzir")
                wait_for(lambda: saved_motion() is (not reduced), "Motion choice was not saved")
                capture("appearance")
                activate_word(window, "Reduzir")
                wait_for(lambda: saved_motion() is reduced, "Motion choice did not toggle back")
                press("Escape")

                # Closing settings restores composer focus, not a hidden control.
                type_text("selection")
                press("Return")
                wait_for(lambda: len(commands("prompt")) == 2, "Composer focus was not restored")
                assert commands("prompt")[-1]["message"] == "draft-effort-keyboard-selection"
                assert saved_motion() is reduced
                print(f"Redesign smoke passed: {width}x{height}, reduced motion={reduced}")
            except Exception:
                capture("failure")
                log.flush()
                log.seek(0)
                print(log.read())
                raise
            finally:
                process.terminate()
                process.wait(timeout=5)
