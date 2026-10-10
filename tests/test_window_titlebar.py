"""Structural regression checks; compositor behavior still needs desktop validation."""
import pathlib
import unittest

SOURCE = (pathlib.Path(__file__).resolve().parents[1] / "src/ui/mod.rs").read_text()


class WindowTitlebarTests(unittest.TestCase):
    def test_conversation_header_has_no_window_controls_or_drag(self):
        header = SOURCE.split("fn conversation_header(", 1)[1].split(
            "fn client_titlebar()", 1
        )[0]
        self.assertNotIn("window_controls()", header)
        self.assertNotIn("start_window_move", header)

    def test_controls_and_drag_are_in_separate_titlebar_children(self):
        titlebar = SOURCE.split("fn client_titlebar()", 1)[1].split(
            "fn window_controls()", 1
        )[0]
        self.assertIn('.id("window-titlebar-drag")', titlebar)
        self.assertIn("window.start_window_move()", titlebar)
        self.assertIn(".child(window_controls())", titlebar)

    def test_titlebar_is_only_added_with_client_frame(self):
        frame = SOURCE.split("if let Some(frame) = window_frame::layer(window) {", 1)[1].split(
            "\n        }", 1
        )[0]
        self.assertIn(".child(client_titlebar())", frame)
        self.assertEqual(SOURCE.count(".child(client_titlebar())"), 1)

    def test_close_keeps_confirmation_action(self):
        controls = SOURCE.split("fn window_controls()", 1)[1].split(
            "fn conversation_state(", 1
        )[0]
        self.assertIn("window.dispatch_action(Box::new(CloseWindow), cx)", controls)


if __name__ == "__main__":
    unittest.main()
