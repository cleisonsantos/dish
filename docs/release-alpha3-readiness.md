# Release readiness: proposed v0.1.0-alpha.3

Snapshot reviewed on 2026-10-09. This is a release assessment, not an
authorization to publish or a promise that subsequent commits are validated.

## Recommendation

**A new Linux prerelease is justified.** Use `v0.1.0-alpha.3`, keeping the
`0.1.0` base version in `Cargo.toml`. There are meaningful usability improvements
since alpha.2, and the current main commit has a successful Linux packaging and
installation pipeline. This is not sufficient evidence for a stable release,
a beta label, or officially supported Windows/macOS packages.

Prefer including PR #25 after review, successful CI and merge: direct selection
and consistent clipboard controls address a fundamental transcript interaction.
Do not hold alpha.3 for every open enhancement. If #25 remains blocked, the
already-merged changes can form a smaller alpha.3, with its notes adjusted.

**Publication is not ready to execute unchanged:** candidate packages still
carry the alpha.2 label. A new version label and a freshly validated final
candidate are required. Do not overwrite the existing alpha.2 release assets.

## Reviewed baseline and delivered changes

Latest published release:
[`v0.1.0-alpha.2`](https://github.com/cleisonsantos/dish/releases/tag/v0.1.0-alpha.2),
targeting `3ee77c7f3d5a5495f8e0ee1b47bcba3bf0fbc36e`.

Reviewed main: `d4a9ce100112f0d2339c67b5108718e199b80b82`.
There are 13 commits after alpha.2, including merge commits; this is not a count
of independent features.

| PR | Status at review | Release relevance |
| --- | --- | --- |
| [#11](https://github.com/cleisonsantos/dish/pull/11) | Merged | Native Windows CI build and executable artifact; not a validated Windows installer or support claim. |
| [#12](https://github.com/cleisonsantos/dish/pull/12) | Merged | Jump-to-end control in the transcript and tighter sent-prompt presentation. |
| [#16](https://github.com/cleisonsantos/dish/pull/16) | Merged | Close the model picker by clicking outside it. |
| [#17](https://github.com/cleisonsantos/dish/pull/17) | Merged | Resizable session navigation panel. |
| [#19](https://github.com/cleisonsantos/dish/pull/19) | Merged | Saved sessions remain reachable in the collapsed rail. |
| [#22](https://github.com/cleisonsantos/dish/pull/22) | Merged | Delete saved sessions through Dish's trash, with confirmation and a 10-second undo affordance. Open sessions must first be closed. No automatic trash purge. |
| [#25](https://github.com/cleisonsantos/dish/pull/25) | Open; CI in progress | Direct selection in rendered Markdown, copy icons, and full command/argument/output inspection and copying. Include only after review and final CI approval. |

`CLAUDE.md` was also added with development, release and concurrent-worktree
rules. Existing functionality predating alpha.2 (such as global UI preference
persistence) should not be presented as newly delivered in alpha.3.

## Evidence actually verified

### Main CI

For exactly the reviewed main commit:

- [Linux run 37968263410](https://github.com/cleisonsantos/dish/actions/runs/37968263410)
  completed successfully: locked Cargo checks/tests, strict Clippy, release-tool
  tests, remapped build, license checks, archive and Debian packaging, tests of
  the real archive, and desktop/startup/settings smoke tests.
- The same Linux run installed the generated `.deb` with `apt`, tested the
  installed `/usr/bin/dish`, and removed the package successfully.
- [Windows run 37968263426](https://github.com/cleisonsantos/dish/actions/runs/37968263426)
  compiled and uploaded an executable successfully. The Windows workflow does
  not run the Linux-equivalent interaction, packaging or installation tests.

### Actual CI artifact

Downloaded `dish-linux-x86_64-candidate` from the Linux run above, rather than
relying only on source or a local binary:

- Both the `.tar.gz` and `.deb` SHA-256 sidecars verified successfully.
- All 12 release-package tests passed with `DISH_TEST_ARCHIVE` pointing to the
  downloaded archive, including the real-candidate license/integrity and isolated
  install/uninstall test. That candidate test was not skipped.
- `BUILD.json` reports version `0.1.0-alpha.2`, target
  `x86_64-unknown-linux-gnu`, minimum glibc `2.39`, 510 dependency packages,
  provisional icon, and lockfile SHA-256
  `7c026be23d3098dd564b1f01cde980dba1185164d675f33826989210a4af5847`.

These checks establish useful packaging evidence for main, not approval to
relabel this existing binary as alpha.3. No new final-artifact Gitleaks scan or
manual Wayland/real-desktop validation was performed for this assessment.

### PR #25

Reported and locally observed on the rebased PR branch: 44 Rust tests, strict
Clippy, release-tool tests, and Linux/Xvfb desktop smoke passed. The expanded
smoke test checks Unicode, word selection, forward/reverse drag between prose
and code, rendered-text versus complete-Markdown copying, keyboard activation of
copy controls, Escape, and pasting back into the composer.

CI for #25 was still in progress at this snapshot. Local checks do not replace
CI of the final merged release commit. The selection is scoped to one Markdown
section, not separate messages or sections separated by tools; it does not
provide selection auto-scroll into offscreen virtualized messages.

## Issue triage

| Issue(s) | Treatment for alpha.3 |
| --- | --- |
| #1, #2, #6, #8, #9 | Closed improvements; describe the post-alpha.2 changes actually included, not every historical feature. |
| #24 | Covered by #25; require its review/CI if included. |
| #23 | Partially addressed by #25. Date/time metadata and remaining criteria stay open. Do not claim the full issue is complete. |
| #10 | Full keyboard control remains incomplete. Keep open and document the limitation; #25 adds only a related subset. |
| #21 | Rich session attention/status/ordering is future work. Existing completion indicators are not the complete proposed model. |
| #18 | Git/worktree/branch context remains future work. |
| #7 | Per-conversation UI preferences remain future work; current global persistence is not this feature. |
| #3 | Language standardization remains incomplete; avoid claiming an entirely Portuguese interface. |
| #4 | Native `.ico`/`.icns` assets are not blockers for a Linux preview. |
| #5 | Linux `.deb` and portable archive already exist; AppImage/MSI/DMG and broader packaging remain incomplete. Clarify or split this issue rather than blocking the Linux release on all formats. |
| #13, #14, #15 | Windows setup, console-window and validation gaps argue against official Windows support in this release. |
| #20 | `CLAUDE.md` documents the policy, but an `AGENTS.md` deliverable is not established by that alone. Review the issue's acceptance criteria; not a runtime release blocker. |

Open enhancements are not automatically release blockers. Newly discovered data
loss, startup failure, transcript corruption, clipboard regression or packaging
failure would be blockers, particularly around saved-session deletion and #25.
This assessment is not an exhaustive code/security audit and does not certify
that such bugs are absent.

## Proposed release scope and notes

Title: **Dish 0.1.0-alpha.3 — Linux usability preview**.

Describe:

- Improved session navigation: resize the panel, access saved sessions when
  collapsed, and delete saved sessions with confirmation/trash/undo.
- Improved transcript navigation: jump to the latest message.
- Model picker dismisses on outside click.
- If #25 merges: select directly in rendered text and use consistent copy icons;
  inspect/copy complete commands, arguments and outputs.

Keep explicit:

- Linux x86_64 preview, glibc 2.39+ for the Ubuntu 24.04 CI artifact.
- Alpha quality; agents operate with the user's file permissions, not in a
  sandbox.
- Windows builds are experimental and not yet officially validated; macOS and
  AppImage/MSI/DMG packages are not supplied by this Linux release.
- Provisional icon, incomplete keyboard coverage/language consistency, and
  limitations of text selection if #25 is included.
- Trashed sessions are preserved separately; automatic purge is not implemented.

## Publication gates

Follow `docs/releasing.md`; publication remains a separate, explicit action.

- [ ] Decide whether #25 is included; review it and require its CI to pass.
- [ ] Update both Linux candidate commands in `.github/workflows/ci.yml` from
      `0.1.0-alpha.2` to `0.1.0-alpha.3` through a reviewed PR. Keep `Cargo.toml`
      at base `0.1.0` unless changing the release line intentionally.
- [ ] Align installation/release examples and release notes with the new label;
      historical audit records should remain historical, not be rewritten.
- [ ] Run all mandatory checks and the entire CI package/installed-binary smoke
      pipeline on the exact final release commit.
- [ ] Download that commit's CI artifact; verify both sidecars, version,
      `BUILD.json`, lockfile hash, glibc requirement and license bundle.
- [ ] Re-run tests with the actual final archive; do not substitute a local
      development build for the published binary.
- [ ] Scan the final Git history and extracted artifact, including bundled
      sources, using Gitleaks with redacted output; inspect the final archive.
- [ ] Manually validate on a real Linux desktop, preferably Ubuntu 24.04 with
      Wayland and X11: startup/setup, restore/preferences, session deletion/undo,
      streaming/scrolling, selection/clipboard, close confirmations and accents.
- [ ] With owner approval, tag the verified commit as `v0.1.0-alpha.3` and publish
      a **prerelease**, attaching the verified `.tar.gz`, `.deb` and sidecars.
- [ ] Download the published assets again and verify their checksums.

Nothing in this assessment merges #25, edits version labels, creates a release
or uploads local build artifacts.
