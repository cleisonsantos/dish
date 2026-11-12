//! Global, session-independent notifications. Producers supply all content.
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;

use gpui::{Context, Window};

/// Pure decision: notify only for a new completion outside the user's view.
pub fn should_notify(enabled: bool, newly_completed: bool, unread_completion: bool) -> bool {
    enabled && newly_completed && unread_completion
}

/// Maximum number of cards painted at once; the rest stay in FIFO order.
pub const MAX_VISIBLE: usize = 2;

/// Actions run inside the workspace listener and receive the workspace directly, so they must never
/// call `update` on the Workspace entity (that re-enters and panics, #148).
pub type NotificationAction = Rc<dyn Fn(&mut crate::workspace::Workspace, &mut Window, &mut Context<crate::workspace::Workspace>)>;

#[allow(dead_code)] // Product producers will use this API in later changes.
pub struct Notification {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub action_label: Option<String>,
    pub action: Option<NotificationAction>,
}

#[derive(Default)]
pub struct NotificationQueue {
    pending: VecDeque<Notification>,
}

impl NotificationQueue {
    pub fn push(&mut self, notification: Notification, dismissed: &BTreeSet<String>) -> bool {
        if dismissed.contains(&notification.id)
            || self.pending.iter().any(|item| item.id == notification.id) {
            return false;
        }
        self.pending.push_back(notification);
        true
    }

    pub fn visible(&self, streaming: bool) -> impl Iterator<Item = &Notification> {
        self.pending.iter().take(if streaming { 0 } else { MAX_VISIBLE })
    }

    pub fn dismiss(&mut self, id: &str) -> bool {
        let Some(index) = self.pending.iter().position(|item| item.id == id) else { return false; };
        self.pending.remove(index);
        true
    }

    pub fn action(&self, id: &str) -> Option<NotificationAction> {
        self.pending.iter().find(|item| item.id == id)?.action.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_unread_new_completions_notify() {
        for enabled in [false, true] {
            for newly_completed in [false, true] {
                for unread_completion in [false, true] {
                    assert_eq!(
                        should_notify(enabled, newly_completed, unread_completion),
                        enabled && newly_completed && unread_completion,
                        "enabled={enabled} newly_completed={newly_completed} unread={unread_completion}"
                    );
                }
            }
        }
        assert!(!should_notify(false, true, true), "desligado nunca notifica");
        assert!(!should_notify(true, false, true), "streaming, histórico e conclusões já contadas não notificam");
        assert!(!should_notify(true, true, false), "sessão à vista nunca notifica");
    }

    #[test]
    fn history_streaming_and_repeated_agent_end_do_not_notify() {
        use crate::session_activity::ResponseTracker;
        use serde_json::json;

        let mut tracker = ResponseTracker::default();
        let mut seen_completed_runs = 0;
        for (kind, record, expected) in [
            ("response", json!({"data":{"messages":[{"role":"assistant","content":"history"}]}}), false),
            ("agent_end", json!({}), false),
            ("agent_start", json!({}), false),
            ("message_update", json!({"assistantMessageEvent":{"type":"text_delta","delta":"partial"}}), false),
            ("message_end", json!({"message":{"role":"assistant","content":[{"type":"text","text":"response"}]}}), false),
            ("agent_end", json!({}), true),
            ("agent_end", json!({}), false),
        ] {
            tracker.observe(kind, &record);
            let newly_completed = tracker.completed != seen_completed_runs;
            assert_eq!(should_notify(true, newly_completed, true), expected, "{kind}");
            seen_completed_runs = tracker.completed;
        }
    }

    fn item(id: &str) -> Notification {
        Notification { id: id.into(), title: id.into(), detail: String::new(), action_label: None, action: None }
    }

    #[test]
    fn fifo_limit_deduplication_and_streaming() {
        let mut queue = NotificationQueue::default();
        for id in ["one", "two", "three", "two"] { queue.push(item(id), &BTreeSet::new()); }
        assert_eq!(queue.visible(false).map(|item| item.id.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(queue.visible(true).count(), 0);
        assert!(queue.dismiss("one"));
        assert_eq!(queue.visible(false).map(|item| item.id.as_str()).collect::<Vec<_>>(), ["two", "three"]);
        queue.push(item("one"), &["one".to_string()].into());
        assert_eq!(queue.visible(false).count(), 2);
    }
}
