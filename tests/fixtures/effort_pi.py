#!/usr/bin/env python3
"""Synthetic asynchronous Pi: distinct model ladders and gated responses."""
import json
import os
from pathlib import Path
import sys
import threading
import time

lock = threading.Lock()
model = "alpha"
level = "low"
switching = False
ladders = {
    "alpha": ["off", "low", "high"],
    "beta": ["off", "minimal", "low", "medium", "high", "xhigh", "max"],
    "gamma": [],
}
root = Path(os.environ["DISH_EFFORT_TEST_DIR"])


def record(value):
    with lock:
        with (root / "events.jsonl").open("a") as stream:
            stream.write(json.dumps(value) + "\n")


def emit(value):
    with lock:
        print(json.dumps(value), flush=True)


def response(command, data=None, success=True):
    emit({"type": "response", "id": command.get("id"), "command": command["type"],
          "success": success, "data": data or {}, "error": None if success else "Synthetic model failure"})


def info(name):
    return {"id": name, "provider": "synthetic", "reasoning": bool(ladders.get(name))}


def wait_gate(name):
    gate = root / name
    if gate.exists():
        record({"waiting": name})
        deadline = time.monotonic() + 60
        while gate.exists() and time.monotonic() < deadline:
            time.sleep(0.01)


def switch(command):
    global model, level, switching
    wait_gate("hold-switch")
    target = command["modelId"]
    if target == "failure":
        switching = False
        response(command, success=False)
        return
    model = target
    level = "xhigh" if target == "beta" else "off" if target == "gamma" else "low"
    switching = False
    emit({"type": "thinking_level_changed", "level": level})
    response(command, info(model))


def levels(command, captured):
    wait_gate("hold-levels")
    if (root / "fail-levels").exists():
        response(command, success=False)
    else:
        response(command, {"levels": ladders[captured]})
        # Deliberately duplicate old data after the next switch is complete.
        if (root / "duplicate-levels").exists():
            wait_gate("hold-duplicate")
            response(command, {"levels": ladders[captured]})
            record({"duplicate_sent": captured})


if "--version" in sys.argv:
    print("1.2.0")
    sys.exit(0)
if "--print" in sys.argv:
    sys.exit(1)

for line in sys.stdin:
    command = json.loads(line)
    kind = command["type"]
    record({"command": command, "model": model, "switching": switching})
    if kind == "set_model":
        switching = True
        threading.Thread(target=switch, args=(command,), daemon=True).start()
    elif kind == "get_state":
        # Let the smoke observe a rejected switch before recovery replaces its
        # persistent error state. No timing dependency on the short-lived toast.
        wait_gate("hold-recovery")
        response(command, {"model": info(model), "thinkingLevel": level,
                 "sessionId": "synthetic-effort", "isStreaming": False})
    elif kind == "get_available_thinking_levels":
        threading.Thread(target=levels, args=(command, model), daemon=True).start()
    elif kind == "get_available_models":
        response(command, {"models": [info(name) for name in [*ladders, "failure"]]})
    elif kind == "get_messages":
        response(command, {"messages": []})
    elif kind == "get_commands":
        response(command, {"commands": []})
    elif kind == "set_thinking_level":
        if command["level"] not in ladders[model]:
            record({"invalid_effort": command["level"], "model": model})
            response(command, success=False)
        else:
            level = command["level"]
            emit({"type": "thinking_level_changed", "level": level})
            response(command)
    else:
        response(command)
