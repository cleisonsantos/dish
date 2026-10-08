#!/usr/bin/env python3
"""Local test fixture: no model, network access, or user files."""
import json
import os
import queue
import sys
import threading
import time

if "--version" in sys.argv:
    print("0.84.4")
    sys.exit(0)

lock = threading.Lock()
session_file = None
if "--session" in sys.argv:
    session_file = sys.argv[sys.argv.index("--session") + 1]
session_id = str(os.getpid())
level = "low"
running = False
prompts = queue.Queue()
dialog_answer = threading.Event()


def emit(record):
    with lock:
        print(json.dumps(record), flush=True)
        if os.environ.get("DISH_FAKE_PI_LOG"):
            with open(os.environ["DISH_FAKE_PI_LOG"], "a") as log:
                log.write(json.dumps({"pid": os.getpid(), "cwd": os.getcwd(), "record": record}) + "\n")


def run(message, images):
    global level
    emit({"type": "agent_start"})
    content = [{"type": "text", "text": message}] + images
    emit({"type": "message_start", "message": {"role": "user", "content": content}})
    emit({"type": "message_end", "message": {"role": "user", "content": content}})
    emit({"type": "message_start", "message": {"role": "assistant", "content": []}})
    emit({"type": "message_update", "assistantMessageEvent": {
        "type": "text_start", "contentIndex": 0}})
    if message == "dialog":
        time.sleep(0.5)
        level = "max"
        emit({"type": "thinking_level_changed", "level": level})
        emit({"type": "extension_ui_request", "method": "input",
              "id": "fake-dialog", "title": "Fake Pi needs input"})
        dialog_answer.wait(10)
    for index in range(5):
        time.sleep(0.5 if message.startswith("long") else 0.2)
        delta = f"{message} / {os.path.basename(os.getcwd())} / {index}\n"
        if message == "table":
            delta = ("Tabela de teste\n\n| Arquivo | Avaliação |\n| --- | --- |\n"
                     "| `dish-icon.svg` | Conceito próprio, mas sobrecarregado. |\n"
                     "| `dish-logo.svg` | Prato em perspectiva, com filete dourado. |\n\n") if index == 0 else ""
        elif message == "long-scroll" and index == 0:
            delta = "\n\n".join(f"Parágrafo de teste {i}: conteúdo para ler sem saltos." for i in range(100)) + "\n\n"
        emit({"type": "message_update", "assistantMessageEvent": {
            "type": "text_delta", "contentIndex": 0,
            "delta": delta}})
    emit({"type": "message_end", "message": {"role": "assistant",
          "content": [{"type": "text", "text": (
              "| Arquivo | Avaliação |\n| --- | --- |\n"
              "| `dish-icon.svg` | Conceito próprio, mas sobrecarregado. |\n"
              "| `dish-logo.svg` | Prato em perspectiva, com filete dourado. |"
          ) if message == "table" else f"Finished {message} in {os.getcwd()}"}]}})
    emit({"type": "agent_end", "messages": []})


def worker():
    global running
    while True:
        message, images = prompts.get()
        running = True
        run(message, images)
        running = False
        emit({"type": "queue_update", "steering": [], "followUp": []})
        emit({"type": "agent_settled"})


threading.Thread(target=worker, daemon=True).start()


for line in sys.stdin:
    command = json.loads(line)
    kind = command["type"]
    if os.environ.get("DISH_FAKE_PI_LOG"):
        with lock:
            with open(os.environ["DISH_FAKE_PI_LOG"], "a") as log:
                log.write(json.dumps({"pid": os.getpid(), "cwd": os.getcwd(), "command": command}) + "\n")
    if kind == "test_exit":
        sys.exit(0)
    data = {"pid": os.getpid(), "cwd": os.getcwd()}
    if kind == "get_state":
        data.update({"sessionId": session_id, "sessionFile": session_file,
                     "sessionName": None, "thinkingLevel": level,
                     "isStreaming": running,
                     "model": {"id": "fake-model", "provider": "test", "reasoning": True}})
    elif kind == "get_available_thinking_levels":
        data["levels"] = ["off", "low", "high", "max"]
    elif kind == "get_available_models":
        data["models"] = [{"id": "fake-model", "provider": "test", "reasoning": True}]
    elif kind == "get_messages":
        data["messages"] = [
            {"role": "user" if index % 2 == 0 else "assistant",
             "content": [{"type": "text", "text": f"Saved conversation message {index}"}]}
            for index in range(24)
        ] if session_file else []
    elif kind == "get_commands":
        data["commands"] = []
    elif kind == "set_thinking_level":
        level = command["level"]
    elif kind == "prompt":
        prompts.put((command["message"], command.get("images", [])))
    elif kind == "extension_ui_response":
        dialog_answer.set()
    emit({"type": "response", "id": command.get("id"), "command": kind,
          "success": True, "data": data})
