#!/usr/bin/env python3
"""Synthetic title generator + RPC server. Only writes temporary test sessions."""
import json
import os
from pathlib import Path
import sys
import time


def log(value):
    with open(os.environ["DISH_TITLE_LOG"], "a") as stream:
        stream.write(json.dumps(value) + "\n")


def emit(value):
    print(json.dumps(value), flush=True)


if "--version" in sys.argv:
    print("1.2.0")
    sys.exit(0)

if "--print" in sys.argv:
    log({"generation": sys.argv[1:]})
    gate = os.environ.get("DISH_TITLE_GATE")
    deadline = time.monotonic() + 15
    while gate and not Path(gate).exists() and time.monotonic() < deadline:
        time.sleep(0.02)
    if os.environ.get("DISH_TITLE_FAIL"):
        print("synthetic private provider diagnostic", file=sys.stderr)
        sys.exit(1)
    emit({"type": "message_end", "message": {
        "role": "assistant", "stopReason": "stop", "content": [
            {"type": "thinking", "thinking": "Not the session title"},
            {"type": "text", "text": "Corrigir nomes das conversas"}]}})
    sys.exit(0)

session = Path(sys.argv[sys.argv.index("--session") + 1])
name = None
for line in session.read_text().splitlines():
    record = json.loads(line)
    if record["type"] == "session_info":
        name = record["name"]

for line in sys.stdin:
    command = json.loads(line)
    log({"command": command})
    kind = command["type"]
    data = {}
    if kind == "get_state":
        data = {"sessionId": "synthetic-title-session", "sessionFile": str(session),
                "sessionName": name, "isStreaming": False, "thinkingLevel": "off",
                "model": {"id": "synthetic", "provider": "test", "reasoning": False}}
    elif kind == "get_messages":
        data = {"messages": []}
    elif kind == "get_available_models":
        data = {"models": [{"id": "synthetic", "provider": "test"}]}
    elif kind == "get_available_thinking_levels":
        data = {"levels": ["off"]}
    elif kind == "get_commands":
        data = {"commands": []}
    elif kind == "set_session_name":
        name = command["name"]
        with session.open("a") as stream:
            stream.write(json.dumps({"type": "session_info", "name": name}) + "\n")
        emit({"type": "session_info_changed", "name": name})
    emit({"type": "response", "id": command.get("id"), "command": kind,
          "success": True, "data": data})
    if kind == "prompt":
        emit({"type": "agent_start"})
        emit({"type": "message_end", "message": {"role": "user", "content": command["message"]}})
        emit({"type": "message_end", "message": {"role": "assistant", "stopReason": "stop",
              "content": [{"type": "text", "text": "Pedido recebido"}]}})
        emit({"type": "agent_end", "messages": []})
        emit({"type": "agent_settled"})
