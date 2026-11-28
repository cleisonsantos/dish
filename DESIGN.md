---
name: Dish
description: Editorial calmo — a conversation with an agent, set like a page.
colors:
  # Noite (id dish-dark): the canonical dark theme.
  bg: "#131614"
  surface: "#171a18"
  surface-2: "#1d211e"
  inset: "#0e100f"
  hover: "#252a27"
  line: "#2a2f2c"
  line-soft: "#212522"
  line-strong: "#3a403c"
  text: "#e9ece7"
  dim: "#b5bcb5"
  faint: "#8e968f"
  accent: "#86c2ac"
  on-accent: "#0f1a16"
  running: "#86c2ac"
  failure: "#e0876a"
  warn: "#d9a441"
  attention: "#e9ece7"
  git-added: "#81b88b"
  git-removed: "#d98282"
  # Papel (id dish-light): the light counterpart.
  light-bg: "#f6f6f2"
  light-surface: "#eeefea"
  light-surface-2: "#ffffff"
  light-inset: "#ecede6"
  light-hover: "#e6e8e1"
  light-line: "#d6d9d0"
  light-line-soft: "#e2e4dc"
  light-line-strong: "#c3c7bc"
  light-text: "#1b1e1c"
  light-dim: "#444a45"
  light-faint: "#5e655f"
  light-accent: "#2e6b58"
  light-on-accent: "#ffffff"
  light-running: "#2e6b58"
  light-failure: "#a8432b"
  light-warn: "#8a6212"
  light-attention: "#1b1e1c"
  # Presets re-derive neutrals and bring their own accent.
  slate-bg: "#1a1d24"
  slate-accent: "#8fb0e8"
  mist-bg: "#f3f1ec"
  mist-accent: "#3e5c8a"
typography:
  prose:
    fontFamily: Source Serif 4 (bundled)
    fontSize: "18px"
    lineHeight: "30px"
  quote:
    fontFamily: Source Serif 4 (bundled), italic
    fontSize: "21px"
    lineHeight: "31px"
  page-title:
    fontFamily: Source Serif 4 (bundled), semibold
    fontSize: "34px"
    lineHeight: "42px"
  display:
    fontFamily: Source Serif 4 (bundled)
    fontSize: "46px"
  text:
    fontFamily: Geist (bundled)
    fontSize: "13px"
    lineHeight: "20px"
  text-sm:
    fontFamily: Geist (bundled)
    fontSize: "12px"
  text-xs:
    fontFamily: Geist (bundled)
    fontSize: "11.5px"
  mono:
    fontFamily: JetBrains Mono, else the first installed monospace
    fontSize: "12.5px"
  code:
    fontFamily: JetBrains Mono, else the first installed monospace
    fontSize: "13px"
    lineHeight: "20px"
layout:
  rail: "56px"
  drawer: "320px (200–420, draggable when docked)"
  drawer-docks-from: "1120px window width"
  reading-column: "712px (680px text + 16px inset each side)"
  margin: "260px"
  inspector: "316px"
rounded:
  control: "8px"
  small: "6px"
  row: "10px"
  composer: "16px"
  pill: "half the height"
spacing:
  s1: "4px"
  s2: "8px"
  s3: "12px"
  s4: "16px"
  s5: "24px"
  s6: "32px"
---

# Design System: Dish

## Overview

**Creative North Star: "Editorial calmo".** A conversation with a coding agent
is read far more than it is operated, so Dish sets it like a page: a serif
title, the request as a pull quote, answers in a reading serif at a
comfortable measure, and the agent's mechanical activity pushed to the margin
like annotations. Chrome is quiet — a thin rail, a breadcrumb, a single accent
per theme — and appears only where it means "you can act here" or "the agent is
alive".

`src/theme.rs` is the implementation authority for tokens. The approved
mockups live in the "Dish Editorial" design canvas (issue #206); the brand mark
lives in `assets/brand/`.

**Key characteristics**

- Paper and ink: a quiet ground, a darker rail, recessed wells for code.
  Separation by tone and hairlines, not by cards.
- One accent per theme. It never encodes reasoning effort; only the effort
  control shows the level's own colour, next to the level's word.
- Reading serif for what people read; Geist for interface; monospace for
  anything a machine produced.
- Activity in the margin when there is room, inline when there is not.
- States (running, failure, warning, new) are always paired with a shape or a
  word.

## Colors

Four built-in themes share one vocabulary: **Noite** (`dish-dark`, canonical
dark), **Ardósia** (`dish-slate`), **Papel** (`dish-light`, canonical light)
and **Névoa** (`dish-mist`). Old ids `dish-dark`/`dish-light` were kept so
saved preferences resolve; the six presets removed in the redesign fall back
to the default for the appearance. User themes (`docs/themes.md`) declare the
roles they want, including `accent`, and inherit the rest.

### Neutrals

- `bg`: the page — conversation, welcome and settings.
- `surface`: the rail and the sessions drawer.
- `surface-2`: the composer and raised controls.
- `inset`: code wells, search fields.
- `hover`: row hover, the active session row, the active rail button.
- `line-soft`, `line`, `line-strong`: hairlines from subtle to firm.
- `text`, `dim`, `faint`: reading text, secondary text, metadata.

### Accent

`accent()` returns the theme's `brand` colour. It marks the send button (filled
only when there is something to send), keyboard focus, the composer border
while focused or while Pi works, the rail mark and the running indicator. `accent_edge()` (42%) and `accent_hover()` are computed
states. **The Accent-Is-Not-Effort Rule:** the reasoning level never recolours the
window or the global accent.

### Effort level

The effort control and its menu use a local per-level ramp (`effort_color`,
`effort_edge` in `src/theme.rs`, issue #222): the control shows only the level's
word in that level's colour, a chevron, and a 1px border in the same hue at 60%.
"desligado" stays neutral (`faint` text, `line` border). Hues follow Pi's
`thinking*` tokens; dark themes lift the lightness, light themes lower it, so
each level reads at 4.5:1 on its theme. In the menu every row uses its level's
colour and the current one keeps the filled marker and "ativo".

### Status

`running` equals the accent ("the agent is alive"); `failure` is rust, `warn`
is ochre; `attention` (new, unread content) is the strongest ink, distinguished
by weight and a filled dot rather than by hue. Theme tests keep running,
failure, warning and attention at ΔE ≥ 25 from each other and readable at
4.5:1 on every surface where they appear.

## Typography

Fonts are bundled (OFL-1.1, `assets/fonts/`) and registered at startup:

- **Source Serif 4** (`theme::FONT_PROSE`): page titles, the request quote,
  assistant prose, the welcome question, settings and drawer headings, the
  composer's writing text.
- **Geist** (`theme::font_ui()`): interface text, controls, metadata.
- **Monospace** (`theme::font_mono()`): code, commands, tool calls, model ids,
  paths, branch names. JetBrains Mono when installed, else the first installed
  monospace from `MONO_FONT_CANDIDATES` — never a proportional fallback.

Hierarchy: `DISPLAY` 46 (welcome only) › `PAGE_TITLE` 34 › `QUOTE` 21 italic ›
`PROSE` 18/30 › `TEXT` 13 › `TEXT_SM` 12 › `TEXT_XS` 11.5 › `TEXT_MICRO` 11.
**The Reading-First Rule:** answers are the largest running text on screen;
metadata never competes with them.

## Layout

- **Rail** (56px, `theme::RAIL_WIDTH`): the mark, sessions, new session,
  search, a running/pending indicator, settings. It never moves.
- **Sessions drawer** (default 320px): docks beside the conversation from
  1120px of window width (`DRAWER_DOCK_WIDTH`) and is resizable there; below
  that it floats over the conversation and closes on an outside click.
- **Conversation**: full bleed, no card. A 52px breadcrumb bar (`pasta /
  título`, status, Details) closes with a `line-soft` hairline.
- **The Reading-Column Rule.** Turns, the composer, banners and the Git footer
  share one centred measure, `READING_WIDTH` (712px). When the remaining width
  fits the column plus `MARGIN_WIDTH` (260px), the agent's activity moves to
  the margin and column + margin centre together (`ui::marginalia`); the
  composer and footer follow the column, not the window centre.
- **Inspector** (316px) opens on demand from the Details button.
- **Settings** is a page beside the rail, not a modal: a serif title over a
  text menu on the left, the section on the right (max 640px). Clicking the
  rail or pressing Esc returns to the conversation.

## Components

- **Page title**: the first message carries the conversation title (serif 34)
  and a context line (`pasta · N mensagens · modelo`); it scrolls with the
  text.
- **Request**: an italic serif pull quote signed "— você, HH:MM". No bubble.
  Copy and metadata appear on hover.
- **Assistant turn**: no model label (it is in the page context line). Status
  appears only while streaming or on error/abort; duration, metadata and copy
  appear on hover.
- **Marginalia**: per tool group, a micro heading ("3 AÇÕES · 2,1 S") and one
  monospace line per call with its status icon; failures in `failure`.
  Clicking a line opens its details inline in the column.
- **Code**: inline code on `code_wash()` (a veil of the text colour); blocks in
  an `inset` well with a small language header and copy.
- **Composer**: a 16px-radius sheet in the column. Serif writing text; below,
  attach (+), model (monospace), the effort level as its coloured word, and on the right Stop
  ("Parar", rust) while Pi works and the circular Send (up arrow) — outlined
  when empty, filled with the accent when there is something to send. The
  character counter appears only with text or when expanded.
  Its menus (`/`, model, effort) float in an overlay layer above the
  transcript — `/` over the composer, model and effort anchored to their
  control — and never change the layout.
- **Welcome**: folder eyebrow, the serif question, the composer, and three
  suggestions as an index of serif lines separated by hairlines.
- **Drawer**: serif "Sessões" title with open-folder and "Nova" pills; filters
  as text tabs with an ink underline; rows without side stripes, a state
  marker (filled dot = open/active/new, ring = saved, icons for pending and
  error) and a state line; row actions (`…`) appear on hover, focus or when
  active.

## Motion

All animation goes through `src/ui/motion.rs` and honours Reduce motion. A
repeating animation runs only while Pi works (the running pulse, the composer
glow); one-shot transitions stay under 220ms.

## Do's and Don'ts

### Do

- Use tokens from `src/theme.rs`; a hex literal in a component is a defect.
- Keep prose in the serif and machine output in monospace.
- Put mechanical detail in the margin or behind a click, never in the way of
  the answer.
- Pair every status colour with an icon or a word.

### Don't

- Don't recolour the global accent by reasoning effort; a level indicator may
  use the level's colour next to the level's word.
- Don't wrap turns or sections in cards, side stripes or shadows.
- Don't add a second filled action next to Send.
- Don't run repeating animations while Pi is idle.
