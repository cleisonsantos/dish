# UI smoke tests

The smoke tests in `tests/` drive a real Dish window under Xvfb. They send X11
XTest keyboard and mouse events, capture windows with ImageMagick, read labels
with Tesseract, and observe the app through a synthetic Pi. They use temporary
projects, configuration and sessions; they do not call a model, reach the
network, or touch your real Pi state.

Use Xvfb even when you have a desktop session: these tests synthesize input, so
running them against your current display would type into whatever has focus.
Every command below runs from the repository root.

## Prerequisites

- Rust toolchain and the Linux build dependencies from
  [the README](../README.md#linux-build-dependencies). The tests launch the
  binary built by Cargo.
- `Xvfb` and `xvfb-run`. `xvfb-run` also needs `xauth` for its temporary
  authorization file. The tests need an X11 display; a Wayland-only session
  without XWayland is not enough.
- X11 and XTest runtime libraries: `libX11.so` and `libXtst.so`. The smoke
  helpers load them through `ctypes` and use XTest for input.
- Tesseract with the `eng` traineddata. The tests call `tesseract` without
  `-l`, so the default language must be available. Check with
  `tesseract --list-langs`; the output must contain `eng`. If the data lives
  outside the default tessdata directory, set `TESSDATA_PREFIX` so that the
  same command lists `eng`.
- ImageMagick's `import` and `convert` commands. `import` captures windows;
  `convert` crops, resizes and converts captures for OCR and pixel probes.
- `xclip`. Only `tests/desktop_smoke.py` calls it, for clipboard assertions.
- Python 3. The tests use only the standard library; no `pip install` is
  required.
- DejaVu fonts (`fonts-dejavu-core` on Debian/Ubuntu). CI uses them as the
  baseline for text layout and OCR; a different font can change wrapping and
  what Tesseract sees.

On Ubuntu 24.04, a working starting point is the package list CI installs for
its UI jobs plus `libx11-6`, which CI gets transitively through `libxtst6`:

```sh
sudo apt install xvfb xauth libx11-6 libxtst6 xclip tesseract-ocr \
  imagemagick fonts-dejavu-core python3 libegl1 libgl1-mesa-dri \
  libvulkan1 mesa-vulkan-drivers
```

If your distribution splits Tesseract language data into separate packages,
install the one that provides `eng`. CI also sets `LIBGL_ALWAYS_SOFTWARE=1` and
a private `XDG_RUNTIME_DIR` on its headless runner. Use the same settings if the
app cannot create a window on a machine without a GPU.

## Build

Each smoke reads `DISH_TEST_BIN` first. Without it, the file uses its own
fallback path. The three blocking smokes use:

| Smoke | Fallback executable | Build before running |
| --- | --- | --- |
| `tests/desktop_smoke.py` | `target/debug/dish` | `cargo build` |
| `tests/startup_smoke.py` | `target/release/dish` | `cargo build --release` |
| `tests/settings_smoke.py` | `target/release/dish` | `cargo build --release` |

Other smokes follow the same pattern; some default to the release binary and a
few to the debug binary. When running several against one build, set
`DISH_TEST_BIN` to that executable instead of relying on the defaults.

`tests/run_ui.sh` has its own warm-up step. It launches `DISH_TEST_BIN`, or
`target/release/dish` when the variable is unset, once for eight seconds. The
warm-up is not a test; its exit status, including a timeout, is ignored. If the
selected binary is not executable, the warm-up is skipped. The warm-up
executable is independent of the per-test fallback; set `DISH_TEST_BIN` to keep
them the same.

To test an extracted archive, point `DISH_TEST_BIN` at its `bin/dish`. For an
installed Debian package, use `/usr/bin/dish-pi`:

```sh
tar -xzf dish-VERSION-linux-x86_64.tar.gz
DISH_TEST_BIN="$PWD/dish-VERSION-linux-x86_64/bin/dish" \
  xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/startup_smoke.py
```

The archive layout and the installed path are described in
[Preparing a Linux candidate](releasing.md).

## Run one smoke

`xvfb-run -a` starts a free Xvfb display and `-s '-screen 0 1280x960x24'` sets
its only screen to the size CI uses.

Desktop smoke, using the debug build:

```sh
cargo build
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/desktop_smoke.py
```

Startup installer and restart discovery, using the release build:

```sh
cargo build --release
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/startup_smoke.py
```

Settings and preference persistence, using the release build:

```sh
cargo build --release
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/settings_smoke.py
```

A passing run exits 0 and prints a pass line near the end, such as
`Desktop smoke passed; screenshot: /tmp/dish-desktop-....png` (the desktop smoke
also runs its metadata scenario afterwards and prints a second pass line),
`Startup installation smoke passed`, or
`Settings basic modal smoke passed`. The screenshots are temporary evidence,
not part of the test.

A failure exits non-zero and prints the assertion that failed, the app log from
the temporary directory, and usually a screenshot path under `/tmp/dish-*.png`.
Inspect those files first.

## Run several

`tests/run_ui.sh` runs each file it receives exactly once, each under its own
`xvfb-run` display:

```sh
bash tests/run_ui.sh tests/startup_smoke.py tests/desktop_smoke.py tests/settings_smoke.py
```

Build both binaries first, or choose one binary with `DISH_TEST_BIN`:

```sh
cargo build --release
DISH_TEST_BIN="$PWD/target/release/dish" \
  bash tests/run_ui.sh tests/startup_smoke.py tests/desktop_smoke.py tests/settings_smoke.py
```

The runner:

- performs the eight-second warm-up described above;
- prints each test inside a `::group::` block and a failing test as
  `::error::`;
- does not stop at the first failure: it runs every listed test and exits 1 if
  any of them failed;
- does not retry. One invocation is one run, and a failure is meant to be
  visible on the first run.

CI separates the same files into two jobs:

- `ui`: the blocking job. Its `Essential UI smokes (blocking)` step runs
  `tests/startup_smoke.py`, `tests/desktop_smoke.py`, and
  `tests/settings_smoke.py`.
- `ui-informational`: the non-blocking job (`continue-on-error: true`). Its
  `Informative UI smokes (non-blocking)` step runs the remaining smokes,
  including search, providers, session activity and notifications.

Both jobs install the Debian package and set `DISH_TEST_BIN=/usr/bin/dish-pi`,
so CI exercises the packaged executable rather than a local `target/` build.

## Anatomy of a smoke

Smoke files are scripts, not `unittest` cases. Name them
`tests/<name>_smoke.py`; they are not matched by the `test_*.py` patterns used
for unit tests.

### Reuse the XTest helpers

Most smokes start by importing the helper block from `tests/desktop_smoke.py`:

```python
from pathlib import Path

helper_path = Path(__file__).with_name("desktop_smoke.py")
helpers = helper_path.read_text().split(
    'with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:')[0]
exec(compile(helpers, str(helper_path), "exec"))
```

The split marker is the line that starts the desktop smoke's own scenario:

```python
with tempfile.TemporaryDirectory(prefix="dish-desktop-") as temp:
```

Everything before it defines the constants, the X11/XTest bindings,
`find_window`, `wait_for`, `click`, `type_text`, the OCR helpers and the trace
helpers. `exec` runs that prefix only; it does not run the desktop scenario. The
marker must stay exactly where it is. Moving or renaming it truncates the helper
block for every smoke that reuses it.

`tests/ui_test_support.py` is different: it contains pure helpers with no X11
or OCR calls (`parse_words`, `visual_lines`, `visible_point` and `UiTrace`).
Import it directly:

```python
from ui_test_support import UiTrace, visible_point
```

Its unit tests are in `tests/test_ui_test_support.py`; keep new pure helpers
there so they can be tested without a display.

### Isolate state

Every smoke runs against temporary configuration and data directories:

- `XDG_CONFIG_HOME` and `XDG_DATA_HOME` point inside a `TemporaryDirectory`;
- `HOME` is set as well when the code under test or a fixture reads it;
- `PI_CODING_AGENT_DIR` points at a synthetic agent directory;
- `DISH_PI_BIN` points at a wrapper that executes `tests/fixtures/fake_pi.py`;
  the wrapper must be executable;
- `WAYLAND_DISPLAY` and `PI_CODING_AGENT_SESSION_DIR` are removed so the app
  uses the X11 session and the temporary agent directory;
- `XDG_SESSION_TYPE=x11`.

The fake Pi is a local JSONL process. It records the commands and events it sees
in `DISH_FAKE_PI_LOG`, and it has variants such as
`tests/fixtures/effort_pi.py` and `tests/fixtures/title_pi.py`. No smoke needs a
real Pi, a model, a provider credential or the network. `tests/settings_smoke.py`
starts a local HTTP server for npm metadata and points `DISH_PI_REGISTRY_URL` at
it; that server is also synthetic.

### Prefer the paint trace to OCR

Set `DISH_TEST_UI_LOG` to a path that does not exist yet before launching Dish.
The app creates it with mode 0600 and writes one JSON object per line for
elements marked with `test_probe`. Each object has `id`, `bounds`, `visible` and
`data`. `visible` is the part inside the painted clip; the helpers treat a node
with less than 4x4 visible pixels as not visible. The app ends every complete
frame with an observation whose id is `frame`. `UiTrace` ignores partial JSONL
writes and commits nodes only at that marker, so closed or virtualised rows
disappear.

```python
env["DISH_TEST_UI_LOG"] = str(temp / "ui.jsonl")
...
UI_TRACES[window] = UiTrace(temp / "ui.jsonl")
wait_ui(window, "composer-input")
```

`wait_ui`, `click_ui`, `reveal_ui` and `save_ui_trace` are part of the helper
block. Use them for app state: whether a panel is open, which session is active,
what the composer draft contains. Use pixel probes or OCR only for facts that
exist only after rendering, such as a colour, a painted theme surface, or a
label that has no probe. Add a probe where the element is built when a fact is
useful to a test; the `TestProbe` helper lives in `src/ui/test_probe.rs`. The
probe is inert unless `DISH_TEST_UI_LOG` is set and never handles commands.

### Register the new smoke

1. Add the file under `tests/<name>_smoke.py`.
2. Add an invocation to exactly one of the two `bash tests/run_ui.sh ...` lines
   in `.github/workflows/ci.yml`:
   - the `ui` job's `Essential UI smokes (blocking)` step if a failure must
     block a merge;
   - the `ui-informational` job's `Informative UI smokes (non-blocking)` step
     otherwise.
3. Run it through the runner locally:

   ```sh
   bash tests/run_ui.sh tests/<name>_smoke.py
   ```

There is no separate test manifest. `run_ui.sh` runs the paths it receives.

## Troubleshooting

### No display or X11 failure

`tests/desktop_smoke.py` asserts `An X11 display is required` when
`XOpenDisplay` returns null. Run it through `xvfb-run`, not directly. If
`xvfb-run` itself fails, install `xvfb` and `xauth` and check that `/tmp` is
writable. A Wayland session without XWayland cannot satisfy the X11 calls in the
helper block.

`Window did not become viewable` means the Dish window was created but never
mapped. Read the app log printed by the test; common causes are a missing
binary, a missing Mesa/EGL driver, or a failed launch. On a headless machine,
start with `LIBGL_ALWAYS_SOFTWARE=1` and a private `XDG_RUNTIME_DIR`.

### `FileNotFoundError: 'xclip'`

`desktop_smoke.py` calls `xclip` for text and image clipboard checks. Without
it, the test fails while setting up or reading the clipboard, before the product
assertion it was trying to make. The message looks like:

```text
FileNotFoundError: [Errno 2] No such file or directory: 'xclip'
```

Install `xclip`. This error says nothing about Dish. The other smokes do not
invoke `xclip` directly.

### OCR does not find text that is on screen

Tesseract is the least reliable part of a smoke. Before changing the test:

- open the failure screenshot under `/tmp/dish-*.png` and confirm the text is
  actually painted;
- run `tesseract --list-langs` and confirm `eng` is available; if it is not,
  fix `TESSDATA_PREFIX`;
- run the same OCR command on the screenshot (`tesseract IMAGE stdout --psm 6`)
  and try the crop or scale the test uses;
- check the font: CI uses `fonts-dejavu-core`; a different system font can wrap
  the label or change glyph shapes enough to break OCR.

If the text is visible but OCR misses it, the defect is in the capture or OCR
parameters, not necessarily in Dish. A missing label in the trace is different:
it usually means the app did not paint the probe, and the test should wait for
the condition rather than for a fixed delay.

### First launch after a build

The first frame after a new build can be slow: the binary and shared libraries
are not in the page cache, and llvmpipe compiles shaders on first use. The
runner's warm-up exists for this. A single smoke run may also need the first
paint to settle; for example, `startup_smoke.py` allows 60 seconds for the
welcome screen on a cold runner. Do not lower those waits or add a retry. If a
first run times out but a later run passes, investigate the machine (memory,
CPU, missing GPU driver) instead of hiding the cold-start cost.

### Failure under load

The waits are wall-clock bounds, and OCR is CPU-heavy. A parallel build or
several suites at once can starve the app's paint loop and the OCR process until
a bounded wait expires. Run one suite at a time and leave headroom. A failure
that appears only under load is still a failure: the runner does not retry, and
adding a retry would hide the condition that caused it.
