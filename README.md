# Dish

A GPU-accelerated desktop interface for the [Pi](https://github.com/earendil-works/pi) coding agent, written in Rust with [GPUI](https://gpui.rs) (the framework behind Zed).

Dish is a *client*, not a reimplementation. It launches `pi --mode rpc` as a child
process, speaks Pi's JSONL protocol over stdin/stdout, and renders the session in a
real window. The same session file, tools, models, extensions, skills and prompt
templates are used — so a conversation can be started here and continued in the
terminal, and vice versa.

```
dish [pi-flags] [path] [prompt...]
```

## Release status

Dish is pre-release software. The initial release target is **Linux x86_64**;
Windows and macOS are not currently supported or validated. Sessions and tools
can modify project files, so review agent actions and keep backups. Prebuilt
Linux previews are published as a Debian/Ubuntu package (`.deb`) and a portable
`.tar.gz` on the releases page.

Linux candidate packaging and validation are documented in
[docs/releasing.md](docs/releasing.md). The GitHub workflow saves candidate
artifacts only; it does not publish Releases automatically.

## Quick start

```bash
cargo run --release                 # open in the current folder
cargo run --release -- ~/code/app   # open in a specific project
cargo run --release -- --continue   # resume the most recent session for a project
cargo run --release -- "explain this codebase"   # open and send a first prompt
```

Pi can be installed through Dish's first-run setup, or provided on `PATH`
(`npm install -g --ignore-scripts @earendil-works/pi-coding-agent`).
Set `DISH_PI_BIN=/path/to/pi` to use a specific binary.

### First-run setup

Dish checks that Pi responds to `--version` before opening a workspace. If it is
missing or cannot run, the setup screen offers retry, a file picker for an
existing executable, official manual instructions, and an option to close.
`DISH_PI_BIN` always takes priority; correct or remove it before choosing a
different executable.

The recommended option opens the **official Pi installer in an external terminal**
on Linux, after explicit consent. The installer downloads and executes Pi's
official script, chooses its own destination, and may request dependencies or
sudo. Complete its steps in the terminal, then click **Tentar novamente**.
Dish also checks common installation locations when its inherited PATH is stale;
use the executable picker if the installer chose another location. If no supported
terminal is available (or on other platforms), copy the official command instead.
Dish does not monitor or cancel the external installer when it closes.

As an alternative, with Node.js 22.19+ and npm available, the npm option installs
`@earendil-works/pi-coding-agent` using npm with `--ignore-scripts`, without sudo,
under `$XDG_DATA_HOME/dish/pi` (default: `~/.local/share/dish/pi`). The screen
shows the command and destination before consent. Installation never runs
automatically. The official installer does not require npm to already be available.
Raw npm output is not shown because it may include private registry information.

The chosen executable is saved in Dish's preferences and reused for new
conversations, without changing your shell PATH or Pi's session storage.
Version detection does not guarantee
full RPC compatibility. Tasks remain tied to Dish; this adds no background
service and does not guarantee cleanup after an uncatchable crash.

### Linux build dependencies

GPUI needs a windowing backend plus a text stack:

```bash
sudo apt install libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libvulkan-dev \
                 libfontconfig-dev libfreetype-dev libssl-dev
```

Wayland and X11 are both enabled in `Cargo.toml`; GPUI picks whichever session is
available. GNOME/Wayland does not provide server-side decorations, so Dish draws
the frame itself there: drag the conversation title to move the window, use the
minimize and close controls in the header, and drag the outer edges to resize.
X11, macOS and Windows keep their native window frame.
Set `DISH_BACKEND=x11` to run through XWayland instead — the system window manager
then draws the decorations and handles resizing like it does for any other app
(ignored on macOS and Windows).

## The interface

Dish is built like an instrument panel rather than a chat app: graphite surfaces,
hairlines instead of cards, monospace for anything a machine produced, and no
decoration that does not carry meaning.

**The accent is the effort meter.** The reasoning level drives the interface
accent using Pi's built-in dark-theme effort colors — slate when reasoning is off,
blue for low/medium, violet for high, magenta for `xhigh`, red for `max` — so the window itself
tells you how hard the model is about to think. It tints the turn spines in the
transcript, the composer's focus border, the send control, the context rail and
the command list. The effort list in the rail paints every level in its own
colour, so the ladder is visible before you pick from it.

### Images in conversations

A standalone Markdown image such as `![Dish](assets/dish-icon.svg)`
shows a local preview in user and assistant messages. PNG, JPEG, WebP, GIF and SVG
are supported. Relative paths use the conversation's project directory; absolute
paths also work. Previews are limited to a 200px box and do not open a window
when clicked. Choose **Abrir externamente** to use the system's default application.

Missing or invalid images retain their caption, path and external-open action.
Remote URLs and data URLs are not loaded automatically. Images embedded within a
paragraph and general clickable Markdown links are not yet supported.

## What it does

- **Streaming transcript** — assistant text is rendered as it arrives, hung off a
  2px spine in the effort accent. Reasoning starts expanded and folds to a
  one-line preview when you click its row; `/thinking-view` chooses whether new
  turns start expanded or collapsed.
- **Tool rows** — every call is a single quiet row: a status light, the tool name,
  a one-line summary of its arguments, elapsed time. Expanding it reveals
  arguments, output and details in a recessed, scrollable block. Bash output
  streams while the command runs.
- **Composer** — a real text field, not a stub: a selection with an anchor,
  grapheme- and word-wise movement, `⇧`-selection, select-all, undo/redo with
  typing coalesced into steps, cut/copy/paste through the system clipboard, and
  mouse support (click to place the caret anywhere in the tray, drag to select,
  double-click for a word, triple-click for a line). It soft-wraps, and grows to
  fit. `⏎` sends, `⇧⏎` inserts a newline. While the agent is working, sending
  *queues* the message as steering; `esc` stops the run.
- **Shell escape** — start a line with `!` to run a command directly in the
  session, exactly like the CLI's `!`.
- **Slash commands** — typing `/` opens a menu of Pi's commands, skills and
  prompt templates alongside Dish's built-ins, filtered as you type. `↑`/`↓` move
  the highlight, `⏎` runs the selection, `⇥` completes it into the composer (for
  commands that take an argument), and `esc` dismisses the menu without touching
  the running turn. Clicking a row does exactly what `⏎` does.
- **Details rail** — label/value pairs for the session, the effort ladder,
  context-window usage, token and cost totals, the pending queue, extension
  status, and one-click new-session/compact/export.
- **Project sessions** — a collapsible left sidebar groups saved Pi conversations
  by their original working directory. Each opened conversation has a separate
  Pi process. Switching back reuses its running state, draft, selection, scroll
  position, tools and queued messages; other conversations keep running.
  Pending extension dialogs appear when you return to their conversation.
  New-session buttons, `ctrl-n` and `/new` open independent conversations.
  Closing a running session or the window asks for confirmation.
- **Model picker** — a floating panel (click the model name in the header or the
  rail), with the model in use floated to the top and the full list scrollable on
  its own.
- **Extension UI** — `notify`, `setStatus`, `setWidget`, `setTitle` and the
  blocking `select` / `confirm` / `input` / `editor` dialogs are all supported.
- **Live status** — compaction, auto-retry and summarization retries surface as
  banners; errors from Pi's stderr become toasts.

### Settings

The navigation's three-dot button opens a window-level **Configurações** modal
with **Aplicativo**, **Atalhos de teclado**, and
**Sobre / Pi** sections. The collapsed navigation retains a settings button.
Click outside, use **Fechar**, or press Esc to close. F1 and `/help` open the keyboard section directly. Session keyboard
shortcuts do not act on the background while settings is open.

Provider login is managed by Pi itself: open Pi in a terminal and use `/login`.
Dish uses the credentials already configured in Pi and provides no integrated
provider login or logout.

Application settings expose the existing details/navigation visibility and
expanded-thinking preferences. These use the existing Dish preferences store,
separate from authentication.

### Keyboard

Paste images directly into the composer with `ctrl-v`: PNG, JPEG, GIF and WebP
are sent to Pi as image content when you send the message (also while steering a
running turn). Images are listed above the composer with a remove action; no
attachment picker is required. The limit is 10 MiB per image. The selected model
must support images. Failed RPC submissions restore their images to the draft.
Other copied files are inserted as quoted local paths for Pi to read, not uploaded
as binary attachments. For file managers exposing only `text/uri-list`, install
`xclip` on X11 or `wl-clipboard` on Wayland. Plain text paste in dialogs and model
search remains text-only.

Markdown tables render with headers, aligned columns and horizontal scrolling
when wider than the conversation. Streaming follows the bottom only until you
scroll up; returning to the bottom resumes following.

| Key | Action |
| --- | --- |
| `⏎` | Send the prompt |
| `⇧⏎` | Insert a newline |
| `esc` | Stop the run, or close the open dialog/menu |
| `ctrl-n` | New session |
| `ctrl-tab` / `ctrl-shift-tab` | Next / previous open conversation |
| `ctrl-w` | Close active conversation (confirms if running; keeps at least one open) |
| `ctrl-k` | Show navigation and focus session search |
| `ctrl-l` | Return focus to the prompt, active input dialog, or model search |
| `ctrl-shift-m` | Open model picker and focus search |
| `F1` | Open keyboard shortcuts in Settings |
| `ctrl-b` | Toggle the details rail |
| `ctrl-shift-b` | Toggle project/session navigation |
| `ctrl-shift-e` | Cycle available model effort levels (wraps after the last) |
| `ctrl-q` | Close the app |
| `ctrl-v` | Paste into the composer (also `ctrl-shift-v`) |
| `↑` `↓` | Move the slash-menu highlight, or the caret |
| `⇥` | Complete the highlighted command |
| `esc` | Dismiss the slash menu, else stop the run |

While the composer has focus:

| Key | Action |
| --- | --- |
| `←` `→` | Move by character (`⇧` to select) |
| `ctrl` `←` `→` | Move by word (`⇧` to select) |
| `home` `end` | Line start and end (`ctrl` for the whole prompt) |
| `ctrl-a` | Select everything |
| `ctrl-c` · `ctrl-x` · `ctrl-v` | Copy · cut · paste |
| `ctrl-z` · `ctrl-⇧z` | Undo · redo |
| `ctrl-⌫` · `ctrl-⌦` | Delete the previous · next word |
| `click` · `drag` · `2x` · `3x` | Place the caret · select · word · line |
| `!` | Run a shell command in the session |
| `/` | Commands, skills and prompt templates |

Any flag Dish does not recognise is passed straight to Pi, so
`dish --thinking max --continue`, `dish --model anthropic/claude-sonnet-4`, or
`dish --no-session` all behave exactly as they do on the command line.

## Identity

The app icon is a **small ivory porcelain plate**, with no text, cake or Pi
symbol. It represents Dish independently of the underlying agent.

```
assets/
  dish-logo.svg       canonical plate artwork — edit this to change the design
  dish-icon.svg       generated SVG copy for the app
  dish-mark.svg       generated SVG copy of the same plate
  dish-icon.png       generated 512px system icon
  build-icon.py       copies the SVG and renders the PNG using rsvg-convert
```

Regenerate with `python3 assets/build-icon.py` (requires `rsvg-convert`).
The local installer uses `dish-icon.png` for the application launcher.

## Architecture

```
src/
  main.rs       window setup, key bindings, CLI argument handling
  rpc.rs        Pi subprocess transport: JSONL framing, command ids, shutdown
  state.rs      the application model: transcript, tools, queue, session, actions
  sessions.rs   read-only, cached discovery of saved session metadata
  workspace.rs  open conversations, project navigation, process lifetimes
  editor.rs     a small multi-line text editor entity (wrapping, cursor, IME)
  markdown.rs   a compact Markdown renderer for assistant output
  theme.rs      palette, typography, and the effort-driven accent
  ui/
    mod.rs      root view, title bar, status bar, shared widgets
    transcript.rs  virtualised message list: prompts, replies, tools, notes
    composer.rs    input box, send/stop, queue chips, slash menu
    inspector.rs   session, model, reasoning, context, commands
    overlays.rs    toasts, extension dialogs, shortcut sheet
```

Design notes:

- **Pi owns the state.** Dish never guesses what the agent is doing; every panel
  is driven by protocol events or command responses. Optimistically rendered user
  messages are reconciled with Pi's echo by matching the pending index.
- **Strict framing.** The RPC stream is split only on `LF`, as the protocol
  requires; Unicode line separators inside JSON strings are preserved.
- **One writer, one reader thread.** Commands are queued to a writer thread;
  stdout is parsed off the UI thread and delivered over an async channel, so a
  busy agent never blocks rendering.
- **The transcript is virtualised** with a tail-following list that remeasures
  only the message being streamed, and stops following as soon as the reader
  scrolls up.
- **The accent lives in one place.** `theme::set_effort()` rewrites a small
  atomic triple that every colour helper reads, so a change of reasoning level
  re-tints the entire window without threading a palette through the tree.

### UI preferences

Dish stores UI preferences in `$XDG_CONFIG_HOME/dish/state.json`, falling back
 to `~/.config/dish/state.json`: details/navigation visibility, collapsed project
 groups, the last active project/session, and window size. Writes run off the UI
 thread and replace the file atomically. Missing or malformed preferences fall
 back to defaults.

Launching `dish` without arguments restores the last project and session if they
 still exist. Explicit arguments take precedence. Restoration loads conversation
 history; it does not resend prompts or restart interrupted agent tasks. Other
 open conversations, drafts, unread indicators, scroll positions and window
 position are not restored. Delete the preferences file to reset the UI.

### Session storage and concurrency

Discovery reads `~/.pi/agent/sessions/`, respecting `PI_CODING_AGENT_DIR`,
`PI_CODING_AGENT_SESSION_DIR`, `--session-dir`, and the current project's/global
`sessionDir` settings. Directories containing sessions opened in Dish are also
indexed. Metadata refresh runs off the UI thread every 15 seconds or with the
refresh button; unchanged files use cached metadata. Grouping uses the JSONL
header's `cwd`, not the encoded storage-directory name. Missing project folders
and process failures are reported without interrupting other conversations.

Only conversations opened in Dish allocate a process. Navigation does not stop
them; closing the window stops its Pi processes. This is not a detached service.
Processes **share project files** when working directories overlap, so concurrent
edits can conflict. Dish prevents duplicate opens inside its own workspace, but
cannot prevent another application from opening the same session file.

### Validation

Run `cargo check --locked`, `cargo test --locked`, and
`cargo clippy --locked --all-targets -- -D warnings`.
Release tooling tests: `python3 -m unittest discover -s tests -p 'test_release_package.py' -v`.
Set `DISH_TEST_ARCHIVE` to a generated archive to include its actual installation test.
Set `DISH_TEST_BIN` to test an extracted candidate executable in the UI smoke tests.
Concurrent-process tests use Python 3 and a local fake Pi, without calling models
or modifying user sessions. Linux desktop smoke test:
`cargo build && xvfb-run -a -s "-screen 0 1280x960x24" python3 tests/desktop_smoke.py`.
It requires Xvfb, X11/XTest libraries, `xclip`, `tesseract`, and ImageMagick's `import` and
`convert` commands. It also checks image clipboard payloads, copied image files,
Markdown tables, and scroll stability during streaming.
First-run installation and restart discovery can be tested without network access:
`cargo build --release && xvfb-run -a -s "-screen 0 1280x960x24" python3 tests/startup_smoke.py`.
Settings, shortcut search and preference persistence:
`xvfb-run -a -s "-screen 0 1280x960x24" python3 tests/settings_smoke.py`
after `cargo build --release`. This smoke test uses only a synthetic Pi and
isolated configuration, not real provider credentials.

## Credits

The text editor in `src/editor.rs` is adapted from the `Editor` in the
[gpui-ce](https://github.com/gpui-ce/gpui-ce) `view_example` (Apache-2.0), with
soft wrapping, vertical movement and imperative text access added.

Licensed under Apache-2.0.
