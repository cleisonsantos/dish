//! Independent activity, attention and reading signals for the session navigator.
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionFilter {
    #[default]
    All,
    NeedsInput,
    Unread,
    Running,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionSignals {
    pub running: bool,
    pub needs_input: bool,
    pub unread: bool,
    pub queued: usize,
    pub error: bool,
    pub interrupted: bool,
    pub saved: bool,
}

impl SessionSignals {
    pub fn matches(self, filter: SessionFilter) -> bool {
        match filter {
            SessionFilter::All => true,
            SessionFilter::NeedsInput => self.needs_input,
            SessionFilter::Unread => self.unread,
            SessionFilter::Running => self.running,
        }
    }

    pub fn labels(self) -> String {
        let mut labels = Vec::new();
        if self.needs_input {
            labels.push("precisa de você".to_owned());
        }
        if self.unread {
            labels.push("resposta nova".to_owned());
        }
        if self.running {
            labels.push("executando".to_owned());
        }
        if self.error {
            labels.push("erro".to_owned());
        }
        if self.interrupted {
            labels.push("interrompida".to_owned());
        }
        if self.queued > 0 {
            labels.push(format!("{} em fila", self.queued));
        }
        if labels.is_empty() {
            labels.push(if self.saved { "salva" } else { "inativa" }.to_owned());
        }
        labels.join(" · ")
    }
}

/// Counts completed live runs with assistant or tool-result content. History loading
/// and bare agent_end events cannot create unread responses.
#[derive(Default)]
pub struct ResponseTracker {
    pending_text: bool,
    pub completed: u64,
}

impl ResponseTracker {
    pub fn observe(&mut self, kind: &str, record: &serde_json::Value) {
        match kind {
            "agent_start" => self.pending_text = false,
            "message_end" if record["message"]["role"] == "assistant" || record["message"]["role"] == "toolResult" => {
                self.pending_text |= !crate::state::message_text(&record["message"])
                    .trim()
                    .is_empty();
            }
            "agent_end" => {
                if self.pending_text {
                    self.completed = self.completed.saturating_add(1);
                }
                self.pending_text = false;
            }
            _ => {}
        }
    }
}

/// Error messages accompanying an explicit abort describe interruption, not error.
pub fn message_outcome(message: &serde_json::Value) -> (bool, bool) {
    let interrupted = message["stopReason"] == "aborted";
    let error = message["stopReason"] == "error"
        || (!interrupted
            && message
                .get("errorMessage")
                .and_then(serde_json::Value::as_str)
                .is_some());
    (error, interrupted)
}

pub fn epoch_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub fn from_epoch_millis(value: u64) -> SystemTime {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(value))
        .unwrap_or(UNIX_EPOCH)
}

/// These are activity times, not execution timestamps or evidence of success.
pub fn relative_time(time: SystemTime, now: SystemTime) -> String {
    let seconds = now.duration_since(time).unwrap_or_default().as_secs();
    match seconds {
        0..=59 => "agora".into(),
        60..=3599 => format!("há {} min", seconds / 60),
        3600..=86399 => format!("há {} h", seconds / 3600),
        _ => format!("há {} d", seconds / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_completed_live_text_creates_unread_content() {
        use serde_json::json;
        let mut tracker = ResponseTracker::default();
        tracker.observe("agent_end", &json!({}));
        tracker.observe(
            "response",
            &json!({"data":{"messages":[{"role":"assistant","content":"history"}]}}),
        );
        assert_eq!(tracker.completed, 0);
        tracker.observe("agent_start", &json!({}));
        tracker.observe(
            "message_end",
            &json!({"message":{"role":"user","content":"prompt"}}),
        );
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 0);
        for reason in ["stop", "aborted", "error"] {
            tracker.observe("agent_start", &json!({}));
            tracker.observe("message_end", &json!({"message":{"role":"assistant","stopReason":reason,"content":[{"type":"text","text":"resposta á"}]}}));
            assert_eq!(
                tracker.completed,
                if reason == "stop" {
                    0
                } else if reason == "aborted" {
                    1
                } else {
                    2
                }
            );
            tracker.observe("agent_end", &json!({}));
        }
        assert_eq!(tracker.completed, 3);
        tracker.observe("agent_start", &json!({}));
        assert_eq!(tracker.completed, 3); // A new run does not consume previous unread content.
    }

    #[test]
    fn tool_output_counts_but_a_tool_call_alone_does_not() {
        use serde_json::json;
        let mut tracker = ResponseTracker::default();
        tracker.observe("agent_start", &json!({}));
        tracker.observe("message_end", &json!({"message":{"role":"assistant","content":[{"type":"toolCall","id":"synthetic","name":"read","arguments":{}}]}}));
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 0);
        tracker.observe("agent_start", &json!({}));
        tracker.observe("message_end", &json!({"message":{"role":"toolResult","content":[{"type":"text","text":"synthetic tool output"}]}}));
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 1);
    }

    #[test]
    fn aborted_error_message_is_not_a_failure() {
        use serde_json::json;
        assert_eq!(
            message_outcome(&json!({"stopReason":"aborted", "errorMessage":"Request aborted"})),
            (false, true)
        );
        assert_eq!(
            message_outcome(&json!({"stopReason":"error", "errorMessage":"Failure"})),
            (true, false)
        );
        assert_eq!(
            message_outcome(&json!({"errorMessage":"Explicit failure"})),
            (true, false)
        );
        assert_eq!(
            message_outcome(&json!({"stopReason":"stop"})),
            (false, false)
        );
        assert_eq!(message_outcome(&json!({})), (false, false));
    }

    #[test]
    fn filters_are_independent() {
        let idle = SessionSignals::default();
        assert!(idle.matches(SessionFilter::All));
        assert!(!idle.matches(SessionFilter::Running));
        let simultaneous = SessionSignals {
            running: true,
            needs_input: true,
            unread: true,
            queued: 2,
            ..idle
        };
        for filter in [
            SessionFilter::All,
            SessionFilter::NeedsInput,
            SessionFilter::Unread,
            SessionFilter::Running,
        ] {
            assert!(simultaneous.matches(filter));
        }
        assert_eq!(
            simultaneous.labels(),
            "precisa de você · resposta nova · executando · 2 em fila"
        );
    }

    #[test]
    fn saved_and_interrupted_are_not_running_or_errors() {
        for signals in [
            SessionSignals {
                saved: true,
                ..Default::default()
            },
            SessionSignals {
                interrupted: true,
                ..Default::default()
            },
        ] {
            assert!(!signals.matches(SessionFilter::Running));
            assert!(!signals.error);
        }
        assert_eq!(
            SessionSignals {
                saved: true,
                ..Default::default()
            }
            .labels(),
            "salva"
        );
    }

    #[test]
    fn relative_time_handles_missing_future_and_old_activity() {
        let now = UNIX_EPOCH + Duration::from_secs(200000);
        assert_eq!(relative_time(now + Duration::from_secs(5), now), "agora");
        assert_eq!(
            relative_time(now - Duration::from_secs(120), now),
            "há 2 min"
        );
        assert_eq!(
            relative_time(now - Duration::from_secs(7200), now),
            "há 2 h"
        );
        assert_eq!(
            relative_time(now - Duration::from_secs(172800), now),
            "há 2 d"
        );
        assert_eq!(from_epoch_millis(epoch_millis(now)), now);
    }
}
