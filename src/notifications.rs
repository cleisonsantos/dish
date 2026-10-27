//! Global, session-independent notifications. Producers supply all content.
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;

use gpui::{Context, Window};

/// Maximum number of cards painted at once; the rest stay in FIFO order.
pub const MAX_VISIBLE: usize = 2;

pub type NotificationAction = Rc<dyn Fn(&mut Window, &mut Context<crate::workspace::Workspace>)>;

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
