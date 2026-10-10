"""Structural regression checks; compositor behavior still needs desktop validation."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "src/ui/mod.rs").read_text()
WORKSPACE = (ROOT / "src/workspace.rs").read_text()


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
        frame = WORKSPACE.split(
            "if let Some(frame) = crate::ui::window_frame::layer(window) {", 1
        )[1].split("\n        }", 1)[0]
        self.assertIn(".child(crate::ui::client_titlebar())", frame)
        self.assertEqual(WORKSPACE.count(".child(crate::ui::client_titlebar())"), 1)
        self.assertNotIn(".child(client_titlebar())", SOURCE)
        self.assertNotIn("window_frame::layer(window)", SOURCE)

    def test_titlebar_precedes_workspace_body_and_is_outside_inspector_layout(self):
        self.assertLess(
            WORKSPACE.index(".child(crate::ui::client_titlebar())"),
            WORKSPACE.index("let mut body = div()"),
        )
        self.assertNotIn("client_titlebar", SOURCE.split("if self.sidebar {", 1)[1].split(
            "// --------------------------------------------------------", 1
        )[0])

    def test_close_keeps_confirmation_action(self):
        controls = SOURCE.split("fn window_controls()", 1)[1].split(
            "fn conversation_state(", 1
        )[0]
        self.assertIn("window.dispatch_action(Box::new(CloseWindow), cx)", controls)


if __name__ == "__main__":
    unittest.main()
