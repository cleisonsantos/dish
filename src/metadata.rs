//! Timestamp provenance and observed durations, independent of GPUI.
use chrono::{DateTime, Local, TimeZone, Utc};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimeOrigin {
    #[default]
    Pi,
    Dish,
}

#[derive(Clone, Debug, Default)]
pub struct Timing {
    pub observed_start: Option<i64>,
    pub observed_end: Option<i64>,
    /// Timestamp of the tool result / bash message, NOT an execution boundary.
    pub recorded: Option<i64>,
    pub elapsed_ms: Option<u64>,
    clock: Option<Instant>,
    command_request: bool,
}

pub fn now_ms() -> Option<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis(),
    )
    .ok()
}

pub fn valid_timestamp(timestamp: i64) -> Option<i64> {
    (timestamp > 0 && DateTime::<Utc>::from_timestamp_millis(timestamp).is_some())
        .then_some(timestamp)
}

impl Timing {
    pub fn start(&mut self) {
        self.start_at(now_ms(), Instant::now());
    }
    pub fn start_command(&mut self) {
        self.command_request = true;
        self.start();
    }
    fn start_at(&mut self, timestamp: Option<i64>, clock: Instant) {
        if self.clock.is_none() {
            self.observed_start = timestamp.and_then(valid_timestamp);
            self.clock = Some(clock);
        }
    }
    pub fn finish(&mut self) {
        self.finish_at(now_ms(), Instant::now());
    }
    fn finish_at(&mut self, timestamp: Option<i64>, clock: Instant) {
        if self.elapsed_ms.is_some() || self.observed_end.is_some() {
            return;
        }
        self.observed_end = timestamp.and_then(valid_timestamp);
        self.elapsed_ms = self
            .clock
            .and_then(|start| clock.checked_duration_since(start))
            .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    }
    pub fn record(&mut self, timestamp: i64) {
        if let Some(timestamp) = valid_timestamp(timestamp) {
            self.recorded = Some(timestamp);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub label: String,
    pub details: String,
}

pub fn duration(ms: u64) -> String {
    let seconds = ms as f64 / 1000.;
    if seconds < 10. {
        format!("{seconds:.1} s")
    } else {
        format!("{seconds:.0} s")
    }
}

fn format_time<T: TimeZone>(timestamp: i64, zone: &T, full: bool) -> Option<String>
where
    T::Offset: std::fmt::Display,
{
    let time =
        DateTime::<Utc>::from_timestamp_millis(valid_timestamp(timestamp)?)?.with_timezone(zone);
    Some(
        time.format(if full {
            "%d/%m/%Y %H:%M:%S%.3f %:z"
        } else {
            "%d/%m %H:%M:%S"
        })
        .to_string(),
    )
}

pub fn describe(created: Option<(i64, TimeOrigin)>, timing: &Timing) -> Option<Metadata> {
    describe_in(created, timing, &Local)
}

fn describe_in<T: TimeZone>(
    created: Option<(i64, TimeOrigin)>,
    timing: &Timing,
    zone: &T,
) -> Option<Metadata>
where
    T::Offset: std::fmt::Display,
{
    let mut lines = Vec::new();
    if let Some((timestamp, origin)) = created {
        if let Some(time) = format_time(timestamp, zone, true) {
            lines.push(format!(
                "{}: {time}",
                match origin {
                    TimeOrigin::Pi => "Mensagem criada pelo Pi",
                    TimeOrigin::Dish => "Mensagem criada no Dish (horário local observado)",
                }
            ));
        }
    }
    for (label, timestamp) in [
        (
            if timing.command_request {
                "Pedido bash enviado pelo Dish"
            } else {
                "Início observado pelo Dish (recebimento do evento)"
            },
            timing.observed_start,
        ),
        (
            if timing.command_request {
                "Resposta bash recebida pelo Dish"
            } else {
                "Fim observado pelo Dish (recebimento do evento)"
            },
            timing.observed_end,
        ),
        (
            "Resultado registrado pelo Pi (não é o início da execução)",
            timing.recorded,
        ),
    ] {
        if let Some(time) = timestamp.and_then(|t| format_time(t, zone, true)) {
            lines.push(format!("{label}: {time}"));
        }
    }
    if let Some(ms) = timing.elapsed_ms {
        lines.push(format!(
            "Duração observada pelo Dish (relógio monotônico): {}",
            duration(ms)
        ));
    }
    let candidates = [
        timing.observed_end.map(|t| (t, "fim", "Dish")),
        timing.recorded.map(|t| (t, "resultado", "Pi")),
        timing.observed_start.map(|t| (t, "início", "Dish")),
        created.map(|(t, origin)| {
            (
                t,
                "mensagem",
                if origin == TimeOrigin::Pi {
                    "Pi"
                } else {
                    "Dish"
                },
            )
        }),
    ];
    let label = candidates
        .into_iter()
        .flatten()
        .find_map(|(timestamp, kind, source)| {
            format_time(timestamp, zone, false).map(|time| format!("{kind} · {time} · {source}"))
        })?;
    Some(Metadata {
        label,
        details: lines.join("\n"),
    })
}

pub fn assistant_status(
    streaming: bool,
    reason: Option<&str>,
    has_error: bool,
) -> (&'static str, bool) {
    match reason {
        Some("aborted") => ("interrompida", false),
        Some("error") => ("erro", true),
        _ if has_error => ("erro", true),
        _ if streaming => ("respondendo", false),
        Some("length") => ("limite de saída", false),
        Some("toolUse") => ("chamada de ferramenta", false),
        Some("deferred") => ("resposta diferida", false),
        Some("stop") => ("resposta encerrada", false),
        _ => ("resposta registrada", false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_format_and_provenance_are_explicit() {
        let zone = chrono::FixedOffset::west_opt(3 * 3600).unwrap();
        let metadata = describe_in(
            Some((1733234400000, TimeOrigin::Pi)),
            &Timing::default(),
            &zone,
        )
        .unwrap();
        assert_eq!(metadata.label, "mensagem · 03/12 11:00:00 · Pi");
        assert!(metadata.details.contains("03/12/2024 11:00:00.000 -03:00"));
        assert!(!metadata.details.contains("Fim observado"));
    }
    #[test]
    fn missing_or_invalid_history_never_gets_now_or_duration() {
        assert!(describe(Some((0, TimeOrigin::Pi)), &Timing::default()).is_none());
        assert!(describe(Some((i64::MAX, TimeOrigin::Pi)), &Timing::default()).is_none());
        let mut timing = Timing::default();
        timing.record(1733234400000);
        let metadata = describe(None, &timing).unwrap();
        assert!(metadata.label.starts_with("resultado"));
        assert!(!metadata.details.contains("Duração"));
        assert_eq!(timing.observed_start, None);
        assert_eq!(timing.observed_end, None);
    }
    #[test]
    fn wall_clock_changes_do_not_corrupt_duration_and_end_is_idempotent() {
        let clock = Instant::now();
        let mut timing = Timing::default();
        timing.start_at(Some(2000), clock);
        timing.finish_at(Some(1000), clock + std::time::Duration::from_millis(1500));
        timing.finish_at(Some(9999), clock + std::time::Duration::from_secs(9));
        assert_eq!(timing.elapsed_ms, Some(1500));
        assert_eq!(timing.observed_end, Some(1000));
        assert_eq!(duration(1500), "1.5 s");
    }
    #[test]
    fn response_end_is_not_task_success_and_abort_is_not_error() {
        assert_eq!(
            assistant_status(false, Some("aborted"), true),
            ("interrompida", false)
        );
        assert_eq!(
            assistant_status(false, Some("stop"), false).0,
            "resposta encerrada"
        );
        assert_eq!(
            assistant_status(false, Some("error"), false),
            ("erro", true)
        );
        assert_eq!(
            assistant_status(false, Some("length"), false).0,
            "limite de saída"
        );
    }
}
