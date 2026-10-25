//! Read-only UI observations for synthetic desktop tests. Inert unless
//! DISH_TEST_UI_LOG is explicitly set before startup; never handles commands.
//! Geometry comes from paint (not speculative layout), including scroll clips.
use gpui::{prelude::*, *};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::{OnceLock, mpsc};

static SINK: OnceLock<Option<mpsc::Sender<Value>>> = OnceLock::new();

/// Called before GPUI starts: file creation and all writes stay off the UI loop.
/// A fresh, private file prevents accidentally overwriting an existing log.
pub fn init() {
    SINK.get_or_init(|| {
        let path = std::env::var_os("DISH_TEST_UI_LOG")?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) => {
                crate::logging::warn(format_args!("dish: DISH_TEST_UI_LOG: {error}"));
                return None;
            }
        };
        let (sender, receiver) = mpsc::channel::<Value>();
        match std::thread::Builder::new().name("dish-test-ui".into()).spawn(move || {
            let mut file = file;
            for observation in receiver {
                let result = serde_json::to_writer(&mut file, &observation)
                    .map_err(std::io::Error::other)
                    .and_then(|()| file.write_all(b"\n"));
                if let Err(error) = result {
                    crate::logging::warn(format_args!("dish: DISH_TEST_UI_LOG write: {error}"));
                    break;
                }
            }
        }) {
            Ok(_) => Some(sender),
            Err(error) => {
                crate::logging::warn(format_args!("dish: DISH_TEST_UI_LOG thread: {error}"));
                None
            }
        }
    });
}

/// Attach an invisible, non-interactive measurement to an existing element.
/// Closures are lazy: normal sessions allocate no ids, JSON, or extra elements.
pub trait TestProbe: ParentElement + Styled + Sized {
    fn test_probe(self, id: impl FnOnce() -> String, data: impl FnOnce() -> Value) -> Self {
        let Some(sender) = SINK.get().and_then(Option::as_ref) else { return self; };
        let sender = sender.clone();
        let id = id();
        let data = data();
        self.child(
            canvas(|_, _, _| {}, move |bounds, _, window, _| {
                let clip = bounds.intersect(&window.content_mask().bounds);
                let rect = |bounds: Bounds<Pixels>| [
                    f32::from(bounds.origin.x), f32::from(bounds.origin.y),
                    f32::from(bounds.size.width), f32::from(bounds.size.height),
                ];
                // send queues bytes for the writer thread; no filesystem I/O here.
                if let Err(error) = sender.send(json!({
                    "id": id, "bounds": rect(bounds), "visible": rect(clip), "data": data,
                })) {
                    crate::logging::warn(format_args!("dish: DISH_TEST_UI_LOG disconnected: {error}"));
                }
            }).absolute().size_full(),
        )
    }
}
impl<T: ParentElement + Styled> TestProbe for T {}

#[cfg(test)]
mod tests {
    use super::TestProbe;
    use std::cell::Cell;

    #[test]
    fn normal_sessions_do_not_evaluate_probe_payloads() {
        // main/init never runs in unit tests, so no test log is configured.
        let called = Cell::new(false);
        let _ = gpui::div().test_probe(
            || { called.set(true); "probe".into() },
            || { called.set(true); serde_json::json!({}) },
        );
        assert!(!called.get(), "probe payloads must stay lazy without a log");
    }
}
