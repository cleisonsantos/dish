//! Serialize model changes and their authoritative state/effort queries.
//! One RPC is in flight; queued changes coalesce to the latest target. IDs,
//! not arrival order, decide which responses may update model-dependent state.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Switch { provider: String, model: String },
    State,
    Levels,
}

#[derive(Default)]
pub struct ModelSync {
    pending: Option<(String, Request)>,
    queued: Option<Request>,
    pub ready: bool,
    pub failed: bool,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub accepted: Option<Request>,
    pub next: Option<Request>,
    pub failed: bool,
}

impl ModelSync {
    pub fn enqueue(&mut self, request: Request) -> Option<Request> {
        self.ready = false;
        self.failed = false;
        if self.pending.is_some() {
            // A switch already schedules a state query after its confirmation.
            // A redundant refresh must not supersede (and hide) its failure.
            if matches!(request, Request::State)
                && self
                    .pending
                    .as_ref()
                    .is_some_and(|(_, pending)| matches!(pending, Request::Switch { .. }))
            {
                return None;
            }
            // A background refresh must not erase an explicit model choice.
            if !matches!(self.queued, Some(Request::Switch { .. }))
                || matches!(request, Request::Switch { .. })
            {
                self.queued = Some(request);
            }
            None
        } else {
            Some(request)
        }
    }

    pub fn track(&mut self, id: String, request: Request) {
        self.pending = Some((id, request));
    }

    pub fn querying(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, request)| matches!(request, Request::State | Request::Levels))
    }

    pub fn owns(&self, id: Option<&str>) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(pending, _)| Some(pending.as_str()) == id)
    }

    pub fn respond(&mut self, id: Option<&str>, success: bool) -> Outcome {
        if !self.owns(id) {
            return Outcome::default();
        }
        let Some((_, request)) = self.pending.take() else {
            return Outcome::default();
        };
        if let Some(next) = self.queued.take() {
            return Outcome {
                next: Some(next),
                ..Outcome::default()
            };
        }
        if !success {
            self.failed = true;
            return Outcome {
                // An earlier, superseded switch may have succeeded. Re-read
                // Pi instead of retaining a model label that is no longer true.
                next: matches!(request, Request::Switch { .. }).then_some(Request::State),
                failed: true,
                ..Outcome::default()
            };
        }
        let next = match request {
            Request::Switch { .. } => Some(Request::State),
            Request::State => Some(Request::Levels),
            Request::Levels => {
                self.failed = false;
                self.ready = true;
                None
            }
        };
        Outcome {
            accepted: Some(request),
            next,
            failed: false,
        }
    }

    pub fn transport_failed(&mut self) {
        self.pending = None;
        self.queued = None;
        self.ready = false;
        self.failed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn switch(model: &str) -> Request {
        Request::Switch {
            provider: "synthetic".into(),
            model: model.into(),
        }
    }

    #[test]
    fn switch_waits_for_confirmation_then_state_then_levels() {
        let mut sync = ModelSync::default();
        assert_eq!(sync.enqueue(switch("b")), Some(switch("b")));
        sync.track("switch".into(), switch("b"));
        assert_eq!(sync.respond(Some("unrelated"), true), Outcome::default());
        assert_eq!(sync.respond(None, true), Outcome::default());
        assert!(!sync.ready);
        assert_eq!(
            sync.respond(Some("switch"), true).next,
            Some(Request::State)
        );
        sync.track("state".into(), Request::State);
        let state = sync.respond(Some("state"), true);
        assert_eq!(state.accepted, Some(Request::State));
        assert_eq!(state.next, Some(Request::Levels));
        assert!(!sync.ready);
        sync.track("levels".into(), Request::Levels);
        assert_eq!(
            sync.respond(Some("levels"), true).accepted,
            Some(Request::Levels)
        );
        assert!(sync.ready);
        assert_eq!(sync.respond(Some("levels"), true), Outcome::default());
    }

    #[test]
    fn rapid_changes_coalesce_and_stale_levels_are_discarded() {
        let mut sync = ModelSync::default();
        sync.track("old-levels".into(), Request::Levels);
        assert_eq!(sync.enqueue(switch("b")), None);
        assert_eq!(sync.enqueue(switch("c")), None);
        assert_eq!(sync.enqueue(Request::State), None);
        let old = sync.respond(Some("old-levels"), true);
        assert_eq!(old.accepted, None);
        assert_eq!(old.next, Some(switch("c")));
        sync.track("c".into(), switch("c"));
        assert_eq!(sync.respond(Some("old-levels"), true), Outcome::default());
        assert_eq!(sync.respond(Some("c"), true).next, Some(Request::State));
    }

    #[test]
    fn failure_never_marks_effort_ready_and_a_refresh_retries() {
        let mut sync = ModelSync::default();
        for request in [switch("b"), Request::State, Request::Levels] {
            sync.track("request".into(), request);
            assert!(sync.respond(Some("request"), false).failed);
            assert!(!sync.ready);
            assert!(sync.failed);
        }
        assert_eq!(sync.enqueue(Request::State), Some(Request::State));
        assert!(!sync.failed);
        sync.track("state".into(), Request::State);
        sync.transport_failed();
        assert_eq!(sync.respond(Some("state"), true), Outcome::default());
        assert!(sync.failed);
    }

    #[test]
    fn level_events_can_refresh_a_pending_snapshot_without_erasing_a_choice() {
        let mut sync = ModelSync::default();
        sync.track("switch".into(), switch("a"));
        assert!(!sync.querying());
        sync.track("levels".into(), Request::Levels);
        assert!(sync.querying());
        sync.enqueue(switch("b"));
        sync.enqueue(Request::State);
        assert_eq!(sync.respond(Some("levels"), true).next, Some(switch("b")));
    }

    #[test]
    fn a_failed_latest_switch_reloads_the_actual_model_after_an_intermediate_success() {
        let mut sync = ModelSync::default();
        sync.track("a".into(), switch("a"));
        sync.enqueue(switch("failure"));
        assert_eq!(sync.respond(Some("a"), true).next, Some(switch("failure")));
        sync.track("failure".into(), switch("failure"));
        let failed = sync.respond(Some("failure"), false);
        assert!(failed.failed);
        assert_eq!(failed.next, Some(Request::State));
        sync.track("actual".into(), Request::State);
        assert_eq!(
            sync.respond(Some("actual"), true).accepted,
            Some(Request::State)
        );
        sync.track("actual-levels".into(), Request::Levels);
        sync.respond(Some("actual-levels"), true);
        assert!(sync.ready);
        assert!(!sync.failed);
    }

    #[test]
    fn a_background_refresh_does_not_hide_a_switch_failure() {
        let mut sync = ModelSync::default();
        sync.track("switch".into(), switch("b"));
        assert_eq!(sync.enqueue(Request::State), None);
        assert!(sync.respond(Some("switch"), false).failed);
    }

    #[test]
    fn a_superseded_failure_does_not_fail_the_latest_choice() {
        let mut sync = ModelSync::default();
        sync.track("old".into(), switch("a"));
        sync.enqueue(switch("b"));
        let response = sync.respond(Some("old"), false);
        assert!(!response.failed);
        assert!(!sync.failed);
        assert_eq!(response.next, Some(switch("b")));
    }

    #[test]
    fn a_new_choice_discards_an_old_state_and_serializes_switches() {
        for request in [Request::State, switch("a")] {
            let mut sync = ModelSync::default();
            sync.track("old".into(), request);
            sync.enqueue(switch("b"));
            let response = sync.respond(Some("old"), true);
            assert_eq!(response.accepted, None);
            assert_eq!(response.next, Some(switch("b")));
        }
    }
}
