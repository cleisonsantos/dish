# Product

<!-- impeccable:product-schema 1 -->

## Platform

Native desktop (Rust/GPUI).

Linux x86_64 is the initial release target. Broader desktop support is a confirmed goal; Windows and macOS are not currently supported or validated. Impeccable's platform vocabulary currently has no native-desktop value; this product must not be treated as web, iOS, or Android.

## Users

Developers already using Pi and developers new to Pi have equal priority. Existing users need a desktop interface that preserves their tools and conversations; new users need clear setup and an understandable path into Pi-powered coding.

## Product Purpose

Dish provides a fast, transparent desktop interface for the Pi coding agent. Success means faithful Pi compatibility, responsive operation, and clear visibility into agent activity without requiring users to work exclusively in a terminal.

## Positioning

Dish is a Pi client, not a reimplementation of the agent. It launches `pi --mode rpc` and uses Pi's JSONL protocol, session files, tools, models, extensions, skills, and prompt templates. Users can continue the same conversation in Dish or the Pi terminal interface.

## Operating Context

- Conversations are associated with project working directories and saved Pi sessions.
- Each conversation opened in Dish has its own Pi process. Switching conversations preserves their running state; closing Dish stops its processes.
- Users compose prompts, inspect streaming responses and tool activity, choose models and effort levels, and steer or stop running work.
- Pi manages provider authentication in its terminal interface. Dish uses existing Pi credentials.
- First-run setup detects Pi and offers explicit, consent-based installation or selection of an existing executable.

## Capabilities and Constraints

- Streaming transcript, expandable reasoning and tool details, Markdown rendering, text selection, and copy controls.
- Multi-project session navigation, independent open conversations, model selection, effort controls, context and cost visibility, and queued steering messages.
- Image clipboard input, local Markdown image previews, slash commands, shell escape, extension dialogs, and persisted UI preferences.
- Pi owns agent state; Dish displays protocol events and responses rather than inventing activity or results.
- Long-running I/O must not block the UI thread. Failures must be visible to users.
- Concurrent conversations can modify shared project files. Dish is not a detached background service and does not guarantee cross-application session exclusivity.
- Installation never runs automatically. External installer actions and destinations must remain clear before consent.
- The application is pre-release. Broader desktop support is a goal, not a current compatibility claim; its sequence and timeline are undecided.

## Brand Commitments

- Product name: Dish, with an identity independent of Pi.
- User-facing interface language: Portuguese. Code identifiers are English.
- The canonical identity asset is a small ivory porcelain plate, without text, cake, or Pi symbols.
- Existing documented interface commitments include meaningful decoration and an effort-driven accent that communicates reasoning level. This record does not define a replacement visual system.

## Evidence on Hand

- `README.md`: documented workflows, capabilities, limitations, release status, and identity.
- `CLAUDE.md`: implementation, responsiveness, privacy, and language constraints.
- `assets/dish-logo.svg`: canonical plate artwork; derived icons live alongside it.
- `src/`: Rust/GPUI implementation and Pi RPC client.
- `tests/`: synthetic-Pi desktop, startup, settings, and release validation.
- No customer testimonials, adoption metrics, or comparative performance claims have been confirmed for this record.

## Product Principles

1. Preserve Pi compatibility and conversation continuity over duplicating agent behavior.
2. Make agent activity and consequential actions transparent.
3. Serve new and established Pi users equally without hiding essential controls.
4. Keep desktop interaction responsive while work runs.
5. Describe platform support and operational limitations honestly.

## Accessibility & Inclusion

Keyboard operation is an existing product capability and must be preserved. Specific accessibility standards, assistive-technology support targets, and additional interface languages remain undecided.
