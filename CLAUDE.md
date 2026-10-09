# CLAUDE.md — working notes for Dish

Dish is a GPUI desktop client for the Pi coding agent (Rust). These notes are for
anyone — human or agent — changing this repository. They assume little Rust
experience.

## Before every commit

```sh
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_release_package.py' -v
```

All four must pass. `--locked` means "use exactly `Cargo.lock`"; never edit the
lockfile by hand. Clippy warnings are treated as errors here — fix the code, do
not silence it with `#[allow]` unless there is a real reason in a comment.

UI smoke tests (synthetic Pi, no network, temporary homes):

```sh
cargo build --release
xvfb-run -a -s '-screen 0 1280x960x24' python3 tests/desktop_smoke.py
```

`DISH_TEST_BIN` overrides the executable under test (useful to test an extracted
release). `DISH_TEST_ARCHIVE` makes the release tests check a real archive.

## Rust habits that matter here

- **No `unwrap()` in application code.** Use `?` with `.context("...")`
  (`anyhow`) or handle the error. `unwrap()` is acceptable in tests and where a
  value is proven present, with a short comment.
- **Do not block the UI thread.** Long work (process I/O, file scanning,
  discovery) runs on `cx.background_executor()` or a dedicated thread, and
  results come back through a channel — see `src/rpc.rs`.
- **Borrow checker friction is normal.** Prefer passing references, collecting
  small owned values, or restructuring into a helper function over cloning whole
  structures. Reach for `.clone()` only when it is genuinely the simplest fix.
- **Ownership across threads:** a closure moved into a thread must own what it
  uses; `Arc`/`Weak` (via `cx.entity().downgrade()`) is how state is shared.
- **Errors are part of the interface.** User-facing failures become a toast or
  banner; never a panic and never a silent failure.
- Read the compiler error fully before changing code: it usually names the exact
  line and the trait or lifetime that is missing.

## Architecture rules

- **Pi owns the state.** Dish renders what the protocol reports. Never guess what
  the agent is doing or optimistically invent tool results.
- **One writer, one reader.** Commands are queued to Pi's stdin from one thread;
  stdout is parsed off the UI thread. Do not add a second writer.
- **Theme is centralised** (`src/theme.rs`). Colours, spacing and sizes come from
  there; do not hardcode hex values in UI files.
- **Design language:** graphite surfaces, hairlines instead of cards, monospace
  for anything a machine produced, no decoration without meaning. The effort
  level drives the accent colour — keep that behaviour.
- **Markdown rendering is deliberately small** (`src/markdown.rs`): only add a
  construct when it actually appears in conversations, and keep streaming text
  safe (incomplete syntax must stay plain text).

## Tests

- Unit tests live next to the code (`#[cfg(test)] mod tests`), end-to-end UI
  tests in `tests/*.py` (Python 3 + Xvfb + XTest).
- A bug fix should come with a test that fails without it. Prefer testing pure
  functions and small helpers; keep UI tests resilient (find elements by text,
  not by fixed pixel coordinates).
- Flaky-looking UI failures are often timing: add a bounded retry or wait for a
  condition instead of a fixed `sleep`.

## Release and packaging

- `scripts/package-linux.py` builds the `.tar.gz`; `scripts/package-deb.py`
  converts that verified archive into the `.deb`. Never publish a locally built
  binary: the CI artifact on Ubuntu 24.04 is the reference build (glibc 2.39).
- Keep the version label in `.github/workflows/ci.yml` in sync with the release
  you intend to publish; the base version lives in `Cargo.toml`.
- The build remaps source paths and rejects personal home paths in the binary —
  keep it that way. `dist/` and `target/` are Git-ignored; never commit them.
- **Licensing:** every dependency must ship its license text. New crates with no
  bundled license need a hash-pinned entry in `licenses/overrides.json`; native
  components go in `licenses/native.json`. The packaging script fails closed, so
  a new dependency without license data will stop the build — that is intended.
- After changing dependencies, re-run the packaging script and review the diff in
  the generated license bundle.

## Privacy and secrets

- No tokens, credentials, real session files or personal paths in the repository.
  Test fixtures use synthetic data and temporary directories.
- Commits use the repository-local noreply identity. Do not change it.
- Before publishing, scan with Gitleaks (`gitleaks git .`) and inspect the final
  archive, not just the source tree.

## Git and parallel work

- `main` receives changes through pull requests, not direct pushes, when someone
  else may be working at the same time.
- One agent/task per **git worktree**; never two writers in the same working
  tree. State your worktree and branch when opening a PR (see issue #20).
- If you find uncommitted changes you did not make, do not overwrite or revert
  them — report them instead.
- The Git identity is shared, so the history does not show who wrote what:
  describe the change clearly in the commit message and PR body.

## Language

The user interface is Portuguese; code identifiers are English. Match the
language already used in the file you are editing (some comments are Portuguese,
some English) instead of rewriting existing text.
