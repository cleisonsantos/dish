"""Structural regression checks for the client title bar.

The Windows path is derived from the `gpui_ce_windows`/GPUI sources; runtime
behavior still needs a real Windows desktop, which this environment does not
have.
"""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "src/ui/mod.rs").read_text()
WORKSPACE = (ROOT / "src/workspace.rs").read_text()
FRAME = (ROOT / "src/ui/window_frame.rs").read_text()
ICONS = (ROOT / "src/ui/icons.rs").read_text()
README = (ROOT / "README.md").read_text()
MAIN = (ROOT / "src/main.rs").read_text()


class WindowTitlebarTests(unittest.TestCase):
    def titlebar(self):
        return SOURCE.split("fn client_titlebar(", 1)[1].split(
            "fn window_controls(", 1
        )[0]

    def controls(self):
        return SOURCE.split("fn window_controls(", 1)[1].split(
            "fn conversation_state(", 1
        )[0]

    def test_conversation_header_has_no_window_controls_or_drag(self):
        header = SOURCE.split("fn conversation_header(", 1)[1].split(
            "fn client_titlebar(", 1
        )[0]
        self.assertNotIn("window_controls(", header)
        self.assertNotIn("start_window_move", header)
        self.assertNotIn("WindowControlArea", header)

    def test_controls_and_drag_are_in_separate_titlebar_children(self):
        titlebar = self.titlebar()
        self.assertIn('.id("window-titlebar-drag")', titlebar)
        self.assertIn("window.start_window_move()", titlebar)
        self.assertIn(".child(window_controls(window))", titlebar)

    def test_drag_area_registers_native_hitbox_without_eating_windows_clicks(self):
        titlebar = self.titlebar()
        self.assertIn(".window_control_area(WindowControlArea::Drag)", titlebar)
        self.assertIn('.when(!cfg!(target_os = "windows")', titlebar)
        # On Windows the OS owns HTCAPTION drag, double-click and system menu;
        # swallowing mouse-down here would prevent the native behavior.
        drag = titlebar.split('.id("window-titlebar-drag")', 1)[1]
        self.assertLess(
            drag.index(".window_control_area(WindowControlArea::Drag)"),
            drag.index('.when(!cfg!(target_os = "windows")'),
        )
        self.assertIn("window.start_window_move()", drag)

    def test_controls_include_minimize_maximize_restore_and_close(self):
        controls = self.controls()
        self.assertLess(
            controls.index('"window-minimize"'),
            controls.index('"window-maximize"'),
        )
        self.assertLess(
            controls.index('"window-maximize"'),
            controls.index('"window-close"'),
        )
        self.assertIn("window.minimize_window()", controls)
        self.assertIn("window.zoom_window()", controls)
        self.assertIn("window.is_maximized()", controls)
        self.assertIn("Icon::Maximize", controls)
        self.assertIn("Icon::Restore", controls)

    def test_control_hitboxes_cover_min_max_close(self):
        controls = self.controls()
        self.assertEqual(controls.count(".window_control_area("), 1)
        self.assertIn(".window_control_area(area)", controls)
        for area in (
            "WindowControlArea::Min",
            "WindowControlArea::Max",
            "WindowControlArea::Close",
        ):
            self.assertIn(area, controls)

    def test_icons_define_maximize_and_restore(self):
        for variant in ("Minimize", "Maximize", "Restore", "Close"):
            self.assertIn(f"    {variant},", ICONS)
        for arm in (
            "Icon::Minimize =>",
            "Icon::Maximize =>",
            "Icon::Restore =>",
            "Icon::Close =>",
        ):
            self.assertIn(arm, ICONS)

    def test_window_scope_on_every_platform_uses_client_frame_where_needed(self):
        self.assertIn(
            'const WINDOWS_DRAWS_NO_NATIVE_TITLEBAR: bool = cfg!(target_os = "windows");',
            FRAME,
        )
        self.assertIn(
            "matches!(decorations, Decorations::Client { .. }) || windows_without_native_bar",
            FRAME,
        )
        self.assertEqual(FRAME.count("pub fn client_side("), 1)
        self.assertIn("fn needs_client_frame(", FRAME)

    def test_linux_still_requests_client_decorations(self):
        self.assertIn('window_decorations: cfg!(target_os = "linux")', MAIN)

    def test_titlebar_is_only_added_with_client_frame(self):
        frame = WORKSPACE.split(
            "if let Some(frame) = crate::ui::window_frame::layer(window) {", 1
        )[1].split("\n        }", 1)[0]
        self.assertIn(".child(crate::ui::client_titlebar(window))", frame)
        self.assertEqual(
            WORKSPACE.count(".child(crate::ui::client_titlebar(window))"), 1
        )
        self.assertEqual(SOURCE.count("fn client_titlebar("), 1)
        self.assertEqual(WORKSPACE.count("window_frame::layer(window)"), 1)
        self.assertNotIn(".child(client_titlebar(", SOURCE)
        self.assertNotIn("window_frame::layer(window)", SOURCE)

    def test_titlebar_precedes_workspace_body_and_is_outside_inspector_layout(self):
        self.assertLess(
            WORKSPACE.index(".child(crate::ui::client_titlebar(window))"),
            WORKSPACE.index("let mut body = div()"),
        )
        self.assertNotIn("client_titlebar", SOURCE.split("if self.sidebar {", 1)[1].split(
            "// --------------------------------------------------------", 1
        )[0])

    def test_close_keeps_confirmation_action(self):
        controls = self.controls()
        self.assertIn("window.dispatch_action(Box::new(CloseWindow), cx)", controls)

    def test_readme_documents_three_controls_and_no_native_windows_frame(self):
        self.assertNotIn(
            "X11, macOS and Windows keep their native window frame", README
        )
        self.assertIn("client title bar", README)
        self.assertIn("caption-less window", README)
        self.assertIn("maximize/restore and close controls", README)
        self.assertIn("X11 and macOS keep their native", README)


if __name__ == "__main__":
    unittest.main()
