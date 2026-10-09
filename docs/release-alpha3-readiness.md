# Release readiness: proposed v0.1.0-alpha.3

Snapshot reviewed on 2026-10-09 (late afternoon). This is a release assessment,
not an authorization to publish or a promise that subsequent commits are
validated.

## Recommendation

**A new Linux prerelease is justified.** Use `v0.1.0-alpha.3`, keeping the
`0.1.0` base version in `Cargo.toml`. Since alpha.2 there are meaningful,
coordinated usability improvements, and main's current commit passes the full
Linux and Windows CI pipelines. This is not sufficient evidence for a stable
release, a beta label, or officially supported Windows/macOS packages.

**The scope is now mandatory, not optional.** The owner decided that this
release includes the completed work for **#21, #23 and #24** as one coherent
usability jump (recorded in
[PR #26 comment](https://github.com/cleisonsantos/dish/pull/26#issuecomment-6086723975)).
Partially merging only one of them would leave the release notes and the
transcript/sidebar behavior inconsistent, so none of the three should be
deferred as "nice to have".

**Publication is not ready to execute:** the candidate commands in
`.github/workflows/ci.yml` still carry the alpha.2 label, PRs #30 (#21) and #31
(#23) are open, and no final integrated commit has been built, scanned or
validated. Do not overwrite the existing alpha.2 release assets.

## Reviewed baseline and candidate changes

Latest published release:
[`v0.1.0-alpha.2`](https://github.com/cleisonsantos/dish/releases/tag/v0.1.0-alpha.2),
targeting `3ee77c7f3d5a5495f8e0ee1b47bcba3bf0fbc36e`.

Reviewed main: `a7988c0eade09f66f840a7f6b8ae4f0f6301b6f7`, which includes the
already-merged alpha.3 work (#25, #28, #29) on top of the previously audited
`d4a9ce100112f0d2339c67b5108718e199b80b82`.

| PR / issue | Status at review | Release relevance |
| --- | --- | --- |
| [#11](https://github.com/cleisonsantos/dish/pull/11) | Merged | Native Windows CI build and executable artifact; not a validated Windows installer or support claim. |
| [#12](https://github.com/cleisonsantos/dish/pull/12) | Merged | Jump-to-end control in the transcript and tighter sent-prompt presentation. |
| [#16](https://github.com/cleisonsantos/dish/pull/16) | Merged | Close the model picker by clicking outside it. |
| [#17](https://github.com/cleisonsantos/dish/pull/17) | Merged | Resizable session navigation panel. |
| [#19](https://github.com/cleisonsantos/dish/pull/19) | Merged | Saved sessions remain reachable in the collapsed rail. |
| [#22](https://github.com/cleisonsantos/dish/pull/22) | Merged | Delete saved sessions through Dish's trash, with confirmation and a 10-second undo affordance. Open sessions must first be closed. No automatic trash purge. |
| [#25](https://github.com/cleisonsantos/dish/pull/25) | Merged at `abc5a7422d3cb1bb4b2078de7eeff2bfba89f76a`; closes #24 | Direct selection in rendered Markdown, consistent clipboard icons, and complete command/argument/output inspection and copying. |
| [#28](https://github.com/cleisonsantos/dish/pull/28) | Merged at `b8acc4eae7c68c52d7fcbdea4a601e33c2d60c4d` | Keyboard navigation in dialogs and the session list. |
| [#29](https://github.com/cleisonsantos/dish/pull/29) | Merged at `a7988c0eade09f66f840a7f6b8ae4f0f6301b6f7` | Reconcile reasoning effort with the model's ladder, fixing the Linux CI failure introduced with #28. |
| [#30](https://github.com/cleisonsantos/dish/pull/30) (implements #21) | Open; CI green on its branch (two Linux jobs and Windows), awaiting review and merge | Independent session activity/attention/read state, filters, stable ordering and persisted read state. |
| [#31](https://github.com/cleisonsantos/dish/pull/31) (implements #23) | Open; local checks pass, CI pending | Timestamp provenance, observed durations, response/tool status semantics and selectable metadata details. |

`CLAUDE.md` was also added with development, release and concurrent-worktree
rules. Existing functionality predating alpha.2 (such as global UI preference
persistence) should not be presented as newly delivered in alpha.3.

## Evidence actually verified

### Main CI

For exactly the reviewed main commit `a7988c0eade09f66f840a7f6b8ae4f0f6301b6f7`:

- [Linux run 37979418089](https://github.com/cleisonsantos/dish/actions/runs/37979418089)
  completed successfully: locked Cargo checks/tests, strict Clippy, release-tool
  tests, remapped build, license checks, archive and Debian packaging, tests of
  the real archive, and desktop/startup/settings smoke tests.
- [Windows run 37979418177](https://github.com/cleisonsantos/dish/actions/runs/37979418177)
  compiled and uploaded an executable successfully. The Windows workflow does
  not run the Linux-equivalent interaction, packaging or installation tests.

The merge of #25 (`abc5a7422d3cb1bb4b2078de7eeff2bfba89f76a`) also had a green
[Linux run 37971851081](https://github.com/cleisonsantos/dish/actions/runs/37971851081)
and
[Windows run 37971851458](https://github.com/cleisonsantos/dish/actions/runs/37971851458).
The intermediate `b8acc4eae7c68c52d7fcbdea4a601e33c2d60c4d` (#28) failed Linux
([run 37966151420](https://github.com/cleisonsantos/dish/actions/runs/37966151420));
#29 corrected it, so only the post-#29 main is relevant release evidence.

### Actual CI artifact

The earlier artifact audit (alpha.2, run 37968263410) remains valid as
historical evidence for that release only: both `.tar.gz` and `.deb` sidecars
verified, all 12 release-package tests passed with `DISH_TEST_ARCHIVE`, and
`BUILD.json` reported glibc 2.39, 510 dependency packages and lock hash
`7c026be23d3098dd564b1f01cde980dba1185164d675f33826989210a4af5847`.

That binary must not be relabeled as alpha.3. A final candidate for the release
commit has not been built or audited yet.

### PR #31 (metadata, #23)

Locally on the rebased branch `feat/transcript-metadata` and then in CI at its
head `18d1840a05ac66107759121f9940eded652a0af7` (base `a7988c0`):

- `cargo check --locked`, `cargo clippy --locked --all-targets -- -D warnings`
  and 52 Rust tests pass.
- The 12 release-package tests pass (1 skipped without `DISH_TEST_ARCHIVE`).
- `desktop_smoke.py` passes end to end, including its second synthetic-Pi pass
  for historical timestamps, cancellation, complete multiline commands and live
  observed duration, with real clipboard verification.
- Timestamp origins and limits are documented in `docs/transcript-metadata.md`;
  the new `chrono` direct dependency resolves to the chrono version already in
  the lockfile, adding no new package.
- CI on that head is fully green: two Linux jobs
  ([run 37982210107](https://github.com/cleisonsantos/dish/actions/runs/37982210107)
  and
  [run 37982216026](https://github.com/cleisonsantos/dish/actions/runs/37982216026),
  including the packaged-install and smoke steps) and the
  [Windows run 37982210017](https://github.com/cleisonsantos/dish/actions/runs/37982210017).
  The metadata work does not complete the full keyboard-coverage backlog (#10).

### PR #30 (session activity, #21)

Reported by its author and visible in the PR body at this snapshot: a testable
`src/session_activity.rs` signal model, response tracker, filters/ordering,
persisted read state, keyboard filter shortcut, narrow-window wrapping,
54 Rust tests plus dedicated `tests/session_activity_smoke.py` with synthetic
Pi. Its head commit `efd69ba` passed Windows and both Linux jobs
([run 37981557787](https://github.com/cleisonsantos/dish/actions/runs/37981557787),
[run 37981557844](https://github.com/cleisonsantos/dish/actions/runs/37981557844),
[run 37981563202](https://github.com/cleisonsantos/dish/actions/runs/37981563202))
in the author's worktree. It still needs review and merge; the integration
check below was done locally against that commit because #21 and #23 touch the
same state and smoke files.

Locally merging #30's head into #31 in a disposable branch resolved one
`tests/desktop_smoke.py` conflict (keeping #30's OCR navigation and paste-retry
with #31's stable-pixel/expose helpers) and auto-merged the rest. On that
integration snapshot: 62 Rust tests, strict Clippy, `desktop_smoke.py`
(including the metadata pass), `session_activity_smoke.py`, `settings_smoke.py`
and `startup_smoke.py` all passed. This is integration evidence, not a
substitute for CI on the final commit.

## Issue triage

| Issue(s) | Treatment for alpha.3 |
| --- | --- |
| #1, #2, #6, #8, #9 | Closed improvements; describe the post-alpha.2 changes actually included, not every historical feature. |
| #24 | Closed by #25 after review and CI. |
| #23 | Implemented by #31; do not claim completion until the PR is reviewed, CI passes and it is merged together with #21/#24. |
| #21 | Implemented by #30; same requirement. Existing completion indicators are not the complete proposed model. |
| #10 | Full keyboard control remains incomplete. Keep open and document the limitation; #25/#28 and the metadata badges add subsets only. |
| #18 | Git/worktree/branch context remains future work. |
| #7 | Per-conversation UI preferences remain future work; current global persistence is not this feature. |
| #3 | Language standardization remains incomplete; avoid claiming an entirely Portuguese interface. |
| #4 | Native `.ico`/`.icns` assets are not blockers for a Linux preview. |
| #5 | Linux `.deb` and portable archive already exist; AppImage/MSI/DMG and broader packaging remain incomplete. Clarify or split this issue rather than blocking the Linux release on all formats. |
| #13, #14, #15 | Windows setup, console-window and validation gaps argue against official Windows support in this release. |
| #20 | `CLAUDE.md` documents the policy, but an `AGENTS.md` deliverable is not established by that alone. Review the issue's acceptance criteria; not a runtime release blocker. |

Newly discovered data loss, startup failure, transcript corruption, clipboard
regression or packaging failure would be blockers, particularly around
saved-session deletion, #21 activity tracking and #23 metadata. This assessment
is not an exhaustive code/security audit and does not certify that such bugs are
absent.

## Proposed release scope and notes

Title: **Dish 0.1.0-alpha.3 — Linux usability preview**.

Describe:

- Session navigation becomes activity-oriented: resizable panel, saved sessions
  in the collapsed rail, deletion with trash/undo, and (#21) independent
  activity/attention/read signals, filters and stable ordering.
- Transcript becomes easier to use and audit: (#24) select rendered text and
  use consistent copy icons; inspect/copy complete commands, arguments and
  outputs; (#23) explicit timestamp provenance, observed durations and honest
  response/tool status without implying task success.
- Model picker dismisses on outside click; jump-to-end control; keyboard
  navigation in dialogs and the session list; reasoning effort reconciled with
  the selected model.

Keep explicit:

- Linux x86_64 preview, glibc 2.39+ for the Ubuntu 24.04 CI artifact.
- Alpha quality; agents operate with the user's file permissions, not in a
  sandbox.
- Windows builds are experimental and not yet officially validated; macOS and
  AppImage/MSI/DMG packages are not supplied by this Linux release.
- Provisional icon, incomplete keyboard coverage/language consistency, and the
  documented selection scope limits (one Markdown section; no auto-scroll into
  offscreen virtualized messages).
- Restored history shows only timestamps Pi actually stored; Dish does not
  fabricate missing historical times or durations.
- Trashed sessions are preserved separately; automatic purge is not
  implemented.

## Publication gates

Follow `docs/releasing.md`; publication remains a separate, explicit action.

- [x] Decide the release scope: #21 + #23 + #24 are mandatory.
- [ ] Review, rebase as needed and merge #30 and #31; resolve the shared
      `src/state.rs` / `tests/desktop_smoke.py` / fake-Pi conflicts so both
      features are validated together at one commit.
- [ ] Re-run every mandatory check and the entire CI package/installed-binary
      smoke pipeline on that exact integrated commit.
- [ ] Update both Linux candidate commands in `.github/workflows/ci.yml` from
      `0.1.0-alpha.2` to `0.1.0-alpha.3` through a reviewed PR. Keep `Cargo.toml`
      at base `0.1.0` unless changing the release line intentionally.
- [ ] Align installation/release examples and release notes with the new label;
      historical audit records should remain historical, not be rewritten.
- [ ] Download that commit's CI artifact; verify both sidecars, version,
      `BUILD.json`, lockfile hash, glibc requirement and license bundle.
- [ ] Re-run tests with the actual final archive; do not substitute a local
      development build for the published binary.
- [ ] Scan the final Git history and extracted artifact, including bundled
      sources, using Gitleaks with redacted output; inspect the final archive.
- [ ] Manually validate on a real Linux desktop, preferably Ubuntu 24.04 with
      Wayland and X11: startup/setup, restore/preferences, session
      activity/read state, session deletion/undo, streaming/scrolling,
      selection/clipboard, metadata badges, close confirmations and accents.
- [ ] With owner approval, tag the verified commit as `v0.1.0-alpha.3` and
      publish a **prerelease**, attaching the verified `.tar.gz`, `.deb` and
      sidecars.
- [ ] Download the published assets again and verify their checksums.

Nothing in this assessment merges #30/#31, edits version labels, creates a
release or uploads local build artifacts.
