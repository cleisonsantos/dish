"""Pure helpers shared by XTest smokes and unit tests; no X11/OCR dependency."""
import json
from pathlib import Path
from typing import NamedTuple


class Word(NamedTuple):
    x: int
    y: int
    width: int
    height: int
    text: str


def parse_words(tsv, scale=2, origin=(0, 0)):
    words = []
    for row in tsv.splitlines()[1:]:
        fields = row.split("\t")
        if len(fields) != 12 or not fields[11].strip():
            continue
        x, y, width, height = (int(fields[i]) for i in (6, 7, 8, 9))
        words.append(Word(x // scale + origin[0], y // scale + origin[1],
                          max(1, width // scale), max(1, height // scale), fields[11]))
    return words


def visual_lines(words, tolerance=8):
    """TSV block/line ids are unreliable for narrow rails. Use visual Y."""
    lines = []
    for word in sorted(words, key=lambda word: (word.y + word.height / 2, word.x)):
        center = word.y + word.height / 2
        for line in lines:
            line_center = sum(w.y + w.height / 2 for w in line) / len(line)
            if abs(center - line_center) <= tolerance:
                line.append(word)
                break
        else:
            lines.append([word])
    return [sorted(line, key=lambda word: word.x) for line in lines]


def visible_point(node):
    """Inside painted clip, never in composer/inspector hiding part of node.

    Aim at the vertical center: a row can grow a few pixels between the paint
    snapshot and the X click (streaming activity line). Clicking its top edge
    then lands on the project header and collapses the group.
    """
    x, y, width, height = node["visible"]
    if width < 4 or height < 4:
        return None
    return round(x + min(6, width / 2)), round(y + height / 2)


class UiTrace:
    """Consume complete paint frames, not cached coordinates from older rows.

    App's final `frame` observation commits nodes painted before it. Closed or
    virtualised nodes disappear. A partial JSONL write cannot expose half frame.
    """
    def __init__(self, path):
        self.path = Path(path)
        self.offset = 0
        self.partial = b""
        self.pending = {}
        self.nodes = {}
        self.generation = 0

    def poll(self):
        if not self.path.exists():
            return self.nodes
        with self.path.open("rb") as stream:
            stream.seek(self.offset)
            chunk = stream.read()
            self.offset = stream.tell()
        lines = (self.partial + chunk).split(b"\n")
        self.partial = lines.pop()
        for line in lines:
            node = json.loads(line)
            self.pending[node["id"]] = node
            if node["id"] == "frame":
                self.nodes = self.pending
                self.pending = {}
                self.generation += 1
        return self.nodes

    def node(self, identity):
        return self.poll().get(identity)

    def matching(self, predicate):
        return next((node for node in self.poll().values() if predicate(node)), None)

    def describe(self):
        return json.dumps({"generation": self.generation, "nodes": self.poll()},
                          ensure_ascii=False, indent=2)
