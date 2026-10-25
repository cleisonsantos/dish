import json
from pathlib import Path
import tempfile
import unittest

from ui_test_support import UiTrace, Word, parse_words, visible_point, visual_lines


def node(identity, **data):
    return {"id": identity, "bounds": [0, 0, 100, 200],
            "visible": [0, 0, 100, 50], "data": data}


class UiTraceTests(unittest.TestCase):
    def test_only_complete_frames_are_visible_and_closed_nodes_expire(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ui.jsonl"
            trace = UiTrace(path)
            self.assertEqual(trace.poll(), {})
            badge = node("tool-call-badge", expanded=True)
            payload = json.dumps(badge).encode() + b"\n"
            frame = json.dumps(node("frame")).encode() + b"\n"
            path.write_bytes(payload[:13])
            self.assertEqual(trace.poll(), {})
            with path.open("ab") as stream:
                stream.write(payload[13:] + frame[:-2])
            self.assertEqual(trace.poll(), {})
            with path.open("ab") as stream:
                stream.write(frame[-2:])
            self.assertEqual(trace.node("tool-call-badge"), badge)
            self.assertEqual(trace.generation, 1)
            with path.open("ab") as stream:
                stream.write(frame)
            self.assertIsNone(trace.node("tool-call-badge"))
            self.assertEqual(trace.generation, 2)

    def test_invalid_complete_record_is_not_swallowed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ui.jsonl"
            path.write_text("not-json\n")
            with self.assertRaises(json.JSONDecodeError):
                UiTrace(path).poll()

    def test_click_uses_visible_clip_not_offscreen_element_center(self):
        clipped = node("details")
        clipped["bounds"] = [600, 600, 280, 260]
        clipped["visible"] = [600, 600, 280, 30]
        # Vertical center of the clipped part remains inside after small shifts.
        self.assertEqual(visible_point(clipped), (606, 615))
        clipped["visible"][3] = 0
        self.assertIsNone(visible_point(clipped))

    def test_identical_labels_keep_distinct_stable_tool_ids(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ui.jsonl"
            path.write_text("".join(json.dumps(item) + "\n" for item in (
                node("assistant-badge", label="fim"),
                node("tool-live-call-badge", label="fim", expanded=False), node("frame"))))
            trace = UiTrace(path)
            self.assertFalse(trace.node("tool-live-call-badge")["data"]["expanded"])
            self.assertEqual(trace.matching(lambda item: item["id"] == "assistant-badge")["data"]["label"], "fim")


class OcrGeometryTests(unittest.TestCase):
    def test_upscaled_crop_coordinates_return_to_window_space(self):
        tsv = "header\n5\t1\t1\t1\t1\t1\t40\t60\t80\t22\t95\tmonotônico\n"
        self.assertEqual(parse_words(tsv, origin=(300, 100)),
                         [Word(320, 130, 40, 11, "monotônico")])

    def test_visual_rows_ignore_tsv_line_ids_and_sort_left_to_right(self):
        words = [Word(60, 101, 40, 10, "saved"), Word(10, 102, 40, 10, "beta"),
                 Word(10, 130, 40, 10, "dialog")]
        self.assertEqual([[word.text for word in line] for line in visual_lines(words)],
                         [["beta", "saved"], ["dialog"]])


if __name__ == "__main__":
    unittest.main()
