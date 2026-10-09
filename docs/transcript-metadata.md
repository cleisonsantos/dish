# Transcript metadata contract (issue #23)

Dish uses Pi's message metadata and observes live protocol events. It does not
write timestamps back into Pi session files, run commands while inspecting them,
or invent execution boundaries for restored history.

## Sources

The installed Pi RPC/JSON and message-type references define:

| Source | Meaning in Dish |
| --- | --- |
| `AgentMessage.timestamp` | Unix milliseconds: when Pi created that message, not when the assistant finished responding. |
| `toolResult.timestamp` | When Pi registered the tool-result message. Not an execution-start timestamp or a measured duration. |
| `bashExecution.timestamp` | When Pi registered the direct bash message. Historical execution boundaries are unavailable. |
| JSONL entry `timestamp` | ISO string on the session entry; distinct from the nested message timestamp. The transcript consumes `get_messages`, not entry timestamps as inferred execution times. |
| Live `message_start` / `message_end` | Receipt times observed on the Dish host. |
| Live `tool_execution_start` / `tool_execution_end` | Receipt times observed on the Dish host; these event shapes do not supply their own timestamps. |
| Direct RPC `bash` submission / response | Request submission and result receipt observed by Dish, not the exact OS process start/exit. |

Reference: Pi's `docs/message-types.md`, `docs/json.md`, `docs/rpc-commands.md`
and `docs/session-format.md` (see the upstream
[documentation](https://github.com/earendil-works/pi/tree/main/packages/coding-agent/docs)).

## Presentation and duration

- Compact badges show event kind, local date/time and **Pi** or **Dish** source.
- Tooltip/details include year, milliseconds and UTC offset, with Portuguese
  labels spelling out what the timestamp means. The system's local timezone is
  used, not a guessed project timezone.
- Click a badge, or focus it and press Enter/Space, to open selectable details;
  Escape closes the badge details. Inspection does not depend on a tooltip.
- The badge prioritizes observed end, Pi result registration, observed start,
  then message creation. All available times remain in its details.
- Zero, missing, invalid or out-of-range message timestamps are unavailable:
  no current date is substituted for restored history.
- Live response and tool durations use `Instant`, not subtraction of Pi and
  Dish wall-clock values. Backward clock adjustments cannot create a negative
  duration. Repeated terminal events do not reset the measured end.
- A response duration covers its message stream, not the entire task or the
  previous prompt plus tools. A tool-group duration is the sum of known tool
  durations, not a parallel group's wall-clock latency. Direct bash duration
  covers the RPC request interval.
- Live observed times/durations are kept in memory. After restarting, only
  metadata available from Pi's history is shown; missing start/end/duration
  is intentionally omitted.
- An optimistic local user message retains its **Dish** provenance when an echo
  has no Pi timestamp; a valid Pi timestamp replaces it with **Pi** provenance.

## Status semantics

Assistant status comes from `stopReason`, streaming and explicit errors:
`respondendo`, `resposta encerrada`, `chamada de ferramenta`, `limite de saída`,
`interrompida`, `erro`, `resposta diferida`, or `resposta registrada` when no
terminal reason is available. An aborted response is not styled as a failure
merely because Pi also supplies an interruption description.

These labels describe a response, never certify that the requested task was
successfully completed. A pending tool call is `não iniciada`, not a queued
steering message; a group containing unstarted calls must not get a success
check mark. Tool failures come from Pi's `isError`. Direct bash cancellation is
shown as `interrompido`; a missing exit code does not imply exit code zero. A
rejected bash RPC request is marked failed rather than left running indefinitely.

`agent_end` ends a low-level run, not necessarily the entire session's automatic
work. This change does not redefine session-level completion or unread behavior;
that belongs to issue #21.

## Inspection, clipboard and keyboard

The already-merged #24/#25 provides direct rendered-text selection, full command
and argument tooltips, expanded selectable commands/arguments/outputs, and shared
copy icons with Portuguese labels and transient feedback. Whole-response copying
preserves Markdown; code copying omits fences; command copying uses the original
command, not its truncated summary. Clipboard feedback acknowledges a write to
GPUI's clipboard API; that API exposes no OS write-failure result to diagnose.
Linux smoke tests verify that another clipboard client receives the exact bytes.

The app's full keyboard-navigation backlog remains in #10; this change does not
claim every surface has been made keyboard-operable.

## Validation

Unit tests cover provenance, fixed-offset/local formatting, missing/invalid
history, restored tool results, monotonic duration/idempotence, response stop
reasons and unstarted/cancelled group status. `desktop_smoke.py` also runs a fresh
synthetic history fixture under UTC, exercises historical cancellation, reads a
complete truncated command through tooltip and selectable details, copies full
historical time metadata without fabricated duration, and checks live tool
start/end/result time and measured duration through the actual clipboard.

The fixtures contain no real sessions, credentials or personal paths. Tests use
isolated configuration and a synthetic Pi, without network/model calls.

`chrono` was already in the lockfile through GPUI. Making it a direct dependency
for local-time formatting adds no new resolved package; packaging still needs to
be rerun and the generated license inventory reviewed for the updated lockfile.
