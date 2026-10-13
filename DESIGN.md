---
name: Dish
description: The Graphite Instrument Panel — quiet, precise desktop controls for Pi.
colors:
  bg: "#0b0e13"
  surface: "#11151c"
  surface-2: "#191e26"
  inset: "#07080b"
  hover: "#1e242d"
  line: "#252b35"
  line-soft: "#1a1f27"
  line-strong: "#333b47"
  text: "#eceef2"
  dim: "#a6aeb8"
  faint: "#858d97"
  on-accent: "#0b0e13"
  effort-off: "#6c767b"
  effort-minimal: "#68808d"
  effort-low: "#5489a4"
  effort-medium: "#6185cc"
  effort-high: "#9776e5"
  effort-xhigh: "#de54c1"
  effort-max: "#fe5462"
  running: "#4fb3a5"
  failure: "#cf5f92"
  warn: "#d8a24a"
  attention: "#2ecc71"
  git-added: "#81b88b"
  git-removed: "#d98282"
  # Variante clara: `system` segue o desktop; os temas embutidos são fixos.
  light-bg: "#eef1f5"
  light-surface: "#f8f9fb"
  light-surface-2: "#ffffff"
  light-inset: "#e3e8ee"
  light-hover: "#e8edf3"
  light-line: "#d5dbe3"
  light-line-soft: "#e2e7ee"
  light-line-strong: "#c3cbd5"
  light-text: "#1a1f26"
  light-dim: "#3f4753"
  light-faint: "#606874"
  light-on-accent: "#ffffff"
  light-effort-off: "#6a7378"
  light-effort-minimal: "#53778b"
  light-effort-low: "#4a778f"
  light-effort-medium: "#4670c3"
  light-effort-high: "#7d54de"
  light-effort-xhigh: "#ca27a8"
  light-effort-max: "#e50c1e"
  light-running: "#1f7a6d"
  light-failure: "#a93a67"
  light-warn: "#8a5c10"
  light-attention: "#187237"
  light-git-added: "#2f6f38"
  light-git-removed: "#a83c3c"
typography:
  title:
    fontFamily: Roboto
    fontSize: "16.5px"
    fontWeight: 500
  body:
    fontFamily: Roboto
    fontSize: "15px"
  text:
    fontFamily: Roboto
    fontSize: "13px"
    lineHeight: "18px"
  text-sm:
    fontFamily: Roboto
    fontSize: "12px"
  text-xs:
    fontFamily: Roboto
    fontSize: "11.5px"
  text-micro:
    fontFamily: Roboto
    fontSize: "11px"
  mono:
    fontFamily: JetBrains Mono
    fontSize: "12.5px"
  code:
    fontFamily: JetBrains Mono
    fontSize: "13px"
    lineHeight: "20px"
rounded:
  control: "8px"
  small: "6px"
  surface: "12px"
  technical: "4px"
  action: "10px"
  keycap: "4px"
spacing:
  s1: "4px"
  s2: "8px"
  s3: "12px"
  s4: "16px"
  s5: "24px"
  s6: "32px"
components:
  send:
    backgroundColor: "{colors.text}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.action}"
    size: "34px"
  send-hover:
    backgroundColor: "{colors.dim}"
  send-disabled:
    backgroundColor: "{colors.line}"
    textColor: "{colors.faint}"
  action:
    textColor: "{colors.dim}"
    typography: "{typography.text-sm}"
    rounded: "{rounded.control}"
    padding: "7px 12px"
  action-hover:
    backgroundColor: "{colors.hover}"
  composer:
    backgroundColor: "{colors.surface-2}"
    rounded: "{rounded.surface}"
  search:
    backgroundColor: "{colors.inset}"
    rounded: "{rounded.technical}"
    padding: "8px 10px"
    height: "38px"
  nav-active:
    backgroundColor: "{colors.surface-2}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "6px 8px 6px 12px"
  keycap:
    backgroundColor: "{colors.inset}"
    textColor: "{colors.dim}"
    typography: "{typography.text-xs}"
    rounded: "{rounded.keycap}"
    padding: "3px 7px"
  user-request:
    backgroundColor: "{colors.surface-2}"
    rounded: "{rounded.surface}"
    padding: "8px 16px"
---

# Design System: Dish

## Overview

**Creative North Star: "The Graphite Instrument Panel"**

Dish is quiet, precise, and restrained. Graphite layers organize the workspace, compact controls expose agent activity, and readable answers take priority over decoration. Depth is felt, not measured: in the dark theme each rising layer carries a touch more blue than the one below; in the light theme it rises toward white while recessed surfaces darken. Hover states read at a glance, and the accent appears only where it means "this is your turn".

This is a record of the existing Rust/GPUI desktop implementation, not a redesign or a web theme. `src/theme.rs` is the implementation authority for shared tokens; component sources own their local exceptions. Browser-preview snippets in `.impeccable/design.json` illustrate native patterns but are not production components. This documentation was extracted from source, not validated against a fresh rendered capture.

The system is a value, not a stylesheet: `Theme` carries every token below, the dark set is canonical, and the light set (`light-*`) re-derives each role for light surfaces. `system`, `dark`, and `light` are the user's choices; nothing about the layout or components changes between them.

**Key Characteristics:**
- Graphite neutrals separated by tone, hairlines, and a rising blue temperature.
- A Pi effort ladder independent of operational status.
- Readable proportional prose and monospace technical content.
- Flat surfaces at rest; structural shadows for floating UI.
- Compact controls, progressive disclosure, and distinct send/stop actions.

## Colors

Graphite neutrals form the workspace; the Pi effort ladder and independent activity/status signals carry meaning. Two built-in themes share this vocabulary: `dish-dark` holds the canonical values above, and `dish-light` carries the `light-*` counterparts. Appearance is a user preference (`system`, `dark`, or `light`) resolved on every render, so switching themes — including following the desktop — never restarts the app.

### Primary

The primary accent is dynamic, not a fixed brand color. `effort-off`, `effort-minimal`, `effort-low`, `effort-medium`, `effort-high`, `effort-xhigh`, and `effort-max` follow Pi's dark-theme reasoning colors: quiet slate through blue, violet, magenta, and coral. Unknown levels and models without reasoning controls fall back to off.

Use the active effort color for composer focus, selected navigation edges, command selection, reasoning controls, and supported accent actions. `accent_edge()` applies 42% opacity; `accent_hover()` moves the accent away from the current background (lighter on dark, darker on light). These are computed states, not additional fixed palette tokens.

`dish-light` keeps each level's hue and lowers its lightness until the level reads at 4.5:1 on white: the light ladder is a documented derivation of the same ramp, not a second set of meanings.

### Neutral

- `bg`: deepest workspace backdrop.
- `surface`: navigation, inspector, and floating panels.
- `surface-2`: composer, user requests, and active rows.
- `inset`: recessed code, search, and technical output.
- `hover`: interactive row hover and keyboard-highlight backgrounds, one step above the raised surface.
- `line-soft`, `line`, `line-strong`: subtle separators through stronger control boundaries.
- `text`, `dim`, `faint`: primary reading text, secondary controls, and metadata.
- `on-accent`: foreground on accent-filled controls (the dark backdrop in dark, white in light).
- Success deliberately reuses `dim`; the light theme darkens it instead of reusing the dark value.

### Independent Status Signals

`running` is sea-glass teal; `failure` is carmine-magenta; `warn` is amber; `attention` is new-content green. Git additions and removals use separate muted green and rose tokens. Status is reinforced by icons, words, and tooltips.

**The Effort-Is-Not-Status Rule.** Reasoning intensity must not stand in for execution, success, or failure. Maximum effort is coral, not an error state.

The theme tests enforce normal-text contrast of at least 4.5:1 across core surfaces and perceptual separation between status colors and effort levels. They do not constitute a complete accessibility audit.

## Typography

**Interface and body font:** Roboto.
**Technical font:** JetBrains Mono.

Proportional text supports reading and interface scanning; monospace identifies code, commands, model identifiers, paths, and technical values. Assistant prose is proportional in the current implementation, despite broader wording in older documentation. Font names are requested by GPUI; this record does not claim bundled fonts or a verified fallback stack.

### Hierarchy

- `title`: medium-weight conversation titles.
- `body`: user prompts and assistant prose.
- `text`: default interface text.
- `text-sm`: controls and secondary rows.
- `text-xs`: metadata, paths, durations, and section labels.
- `text-micro`: sparingly used technical micro-labels.
- `mono`: tool names and argument summaries.
- `code`: code-block text with its own line height.

There is no shared marketing display scale. Markdown headings are content-renderer styles, not replacements for the interface hierarchy. Most labels use normal case; the code-language label is uppercase with local letter spacing (0.9px).

**The Reading-First Rule.** Keep answers visually larger than the metadata and controls surrounding them; do not turn all assistant output into monospace.

## Layout

The desktop shell combines a project/session navigation pane, a flexible conversation column, and an optional details inspector. The transcript consumes remaining height; the composer and Git context footer remain below it. Navigation and inspector have independent scrolling.

Shared spacing is the six-step `s1`–`s6` scale. Conversation sections and composer use outer horizontal insets (24px); composer writing content uses internal insets (16px). Component-specific offsets are observed exceptions, not a new universal scale.

The inspector is fixed-width (316px). On first initialization it opens for viewport widths at or above (1120px), unless a saved preference already supplies the choice. Below that threshold it overlays the conversation instead of squeezing it. Later toggles remain user-controlled.

Navigation is draggable within (200–420px). Settings width is viewport width minus (48px), capped at (840px), with a local menu width (184px). These are desktop adaptation rules; no mobile-web layouts are implemented or promised here.

The composer grows until its editor reaches (220px), then scrolls. Model lists cap at (300px); code blocks cap vertically at (420px). Wide Markdown tables scroll horizontally. The transcript is virtualized and follows the tail only while the reader has not scrolled away.

## Elevation & Depth

Flat tonal layers and hairlines convey depth at rest. Floating menus, dialogs, inspector overlays, tooltips, and the jump-to-end control can use the shared two-layer overlay shadow. Keyboard focus outlines are a separate structural cue, not surface elevation.

### Shadow Vocabulary

- `shadow_overlay()`: black layers with offsets/blur (0px 18px / 48px) and (0px 2px / 8px); exact alpha values are in the sidecar.
- `focus_ring()`: an external outline with zero blur and spread (1px), using `faint` at 90% opacity.

Scrim opacity varies by context: narrow inspector (72%), extension modal (78%), and workspace-close confirmation (85%). Do not normalize these as part of documentation.

**The Flat-At-Rest Rule.** Reserve structural shadows for floating UI; ordinary panels and transcript containers separate by tone and hairlines.

## Shapes

Shared control and surface radii are distinct. Compact controls use `control`; the composer and user requests use `surface`; menu rows, option rows and inner wells use `small`. Preserve local exceptions: recessed technical fields/code (4px), send/stop actions (10px), keycaps (4px).

Most separators and control strokes are hairlines (1px); active-navigation and semantic banner edges use (2px). Inner cards, menu rows and technical blocks use `small` (6px); recessed fields keep `technical` (4px); keycaps are 4px. Small indicators are softened squares rather than universal circular bullets. Circular geometry exists where functional: switch thumbs and the jump-to-end control.

## Components

### Buttons

Refined and restrained, with local variants rather than one universal primary button.

- **Send:** inverse neutral fill (the `text` color) with `on-accent` foreground, compact square; hover shifts to `dim`. Empty/loading draft uses `line` and `faint`.
- **Stop:** a separate outlined control with failure-colored icon; hover uses a 14% failure wash. It appears while streaming and never replaces Send.
- **Inspector action:** compact outlined text button; hover raises the background and strengthens the border.
- **Extension confirmation:** active effort fill with `on-accent` foreground; hover uses computed `accent_hover()`.
- **Focus:** handling varies by component. Copy controls and navigation support explicit keyboard focus treatments; do not imply every clickable container already has equivalent keyboard support.

### Chips

Keycaps use an inset background, fine border, dim monospace label, and a small radius. Conversation-state badges combine an icon and status word on a raised surface. Queued work is currently a collapsible list with labeled rows, not a collection of decorative pills.

### Cards / Containers

User requests occupy quiet raised containers rather than speech bubbles. Assistant responses remain open on the conversation background, introduced by compact model/status/time headers. Consecutive tool calls group into expandable activity; technical details recess beneath their rows.

The roomy composer is the principal writing surface. Floating picker panels use `surface`, a strong hairline, the surface radius, and overlay shadows. Do not apply floating-panel styling to every content section.

### Inputs / Fields

Composer focus changes the outer border to `accent_edge()`; the raised background remains stable. Model-search and extension-input fields are recessed with fine accent-edge strokes. Technical field heights and local radii differ from the composer.

Model and effort controls sit inside the composer near the input they affect. Slash, model, and effort menus appear above it. Preserve text-selection and editor behaviors instead of substituting static text styled like a field.

### Navigation

Project groups contain session rows with truncated titles and supporting status information. Active rows use `surface-2` and an effort-colored left edge. Unread inactive rows use a green wash and edge; pending input gives the amber edge priority. Keyboard selection adds an external focus outline, separate from those signals.

Collapsed navigation retains compact markers and contextual tooltips. Keep unread, running, pending, active, and saved states distinguishable by more than color alone.

### Signature Components

- **Effort selector:** the Pi ladder shows each available level in its own color; selected levels gain a marker and active label.
- **Activity/tool row:** technical name and summary, elapsed metadata, explicit status icon/word, copy action, and expandable details.
- **Context meter:** a thin track (3px); neutral below or equal to 60%, amber above 60%, and failure-colored above 85%.
- **Live marker:** a repeating opacity pulse (1400ms), from 0.35 to 1.0 using a sine wave. This is activity feedback, not decorative motion; no global easing or reduced-motion policy is established by the inspected sources.

## Do's and Don'ts

### Do:
- **Do** use shared theme tokens and preserve documented local component exceptions.
- **Do** separate reasoning intensity from operational status.
- **Do** prioritize readable answers and reveal technical detail on demand.
- **Do** retain independent send and stop controls.
- **Do** pair consequential status colors with words or recognizable icons.
- **Do** preserve tonal depth, hairline boundaries, and visible keyboard focus cues.

### Don't:
- **Don't** turn every section into a raised or shadowed card.
- **Don't** use maximum-effort coral as the failure signal.
- **Don't** replace proportional answer text with blanket monospace styling.
- **Don't** infer a mobile layout, universal focus treatment, or accessibility certification from this record.
- **Don't** reintroduce effort-colored transcript spines or an effort-filled send button based on outdated README descriptions.
