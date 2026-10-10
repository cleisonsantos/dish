//! Turning the latest thing the user asked into a short conversation title.
//!
//! Pi only rewrites a session name when someone sets one explicitly. For an
//! unnamed session Dish therefore derives the title from the transcript, and
//! the *current* demand is more useful than the first one: the same session
//! often moves on to a different task.
//!
//! When the user opts in, Dish may instead ask the session's model for a short
//! title. That runs in an isolated one-shot process and never touches the Pi
//! session: the result lives only in memory. The literal demand is always the
//! deterministic fallback.

use crate::state::ModelInfo;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// A follow-up like "sim", "ok" or "continue" should not rename the session.
const MIN_WORDS: usize = 3;
const MIN_CHARS: usize = 16;

/// How long the isolated title process may take before it is killed.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fixed instruction. Only the demand is appended, so the cost per round stays
/// tiny and the same demand tends to produce the same title.
const INSTRUCTION: &str = "Escreva um título curto para esta demanda de programação. \
Responda apenas com o título, no idioma da demanda, sem aspas, sem ponto final, \
com no máximo 6 palavras.";

/// Flags that keep the one-shot from loading tools or project resources. No
/// session is written, so this never touches the conversation.
const ISOLATED_FLAGS: [&str; 8] = [
    "--print",
    "--no-session",
    "--no-tools",
    "--no-extensions",
    "--no-mcp",
    "--no-skills",
    "--no-prompt-templates",
    "--no-context-files",
];

/// Collapse all whitespace runs into single spaces, dropping leading and
/// trailing space. Keeps a title on one line.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a message carries enough substance to name the current demand.
///
/// Deliberately allocation-free: this runs while rendering, scanning backwards
/// through a conversation until something worth showing turns up.
pub fn is_substantive(text: &str) -> bool {
    let mut words = 0;
    let mut chars = 0;
    let mut in_word = false;
    for character in text.chars() {
        if character.is_whitespace() {
            in_word = false;
        } else {
            chars += 1;
            if !in_word {
                words += 1;
                in_word = true;
            }
        }
    }
    words >= MIN_WORDS && chars >= MIN_CHARS
}

/// A single-line title clipped to `max_chars`, or `None` when there is nothing
/// to show.
pub fn clip(text: &str, max_chars: usize) -> Option<String> {
    let normalized = normalize(text);
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.chars().take(max_chars).collect())
}

/// Arguments for the isolated title process, in a fixed order. Deterministic
/// on purpose: the same model and demand always produce the same command.
pub fn arguments(model: &ModelInfo, demand: &str) -> Vec<String> {
    let mut args: Vec<String> = ISOLATED_FLAGS.iter().map(|flag| flag.to_string()).collect();
    if !model.provider.is_empty() {
        args.push("--provider".into());
        args.push(model.provider.clone());
    }
    if !model.id.is_empty() {
        args.push("--model".into());
        args.push(model.id.clone());
    }
    args.push(format!("{INSTRUCTION}\n\n{demand}"));
    args
}

/// Reduce the model's answer to a usable single-line title.
///
/// Rejects anything that is not exactly one non-empty line, so an explanation
/// around the title falls back to the literal demand instead of leaking into
/// the UI. Quotes, list markers and a `Title:` prefix are stripped.
pub fn sanitize(raw: &str, max_chars: usize) -> Option<String> {
    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let line = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let trimmed = line
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '“' | '”' | '‘' | '’'))
        .trim();
    let without_label = trimmed
        .strip_prefix("Título:")
        .or_else(|| trimmed.strip_prefix("Title:"))
        .map(str::trim)
        .unwrap_or(trimmed);
    let cleaned = without_label
        .trim_end_matches(['.', ':', ';', ','])
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    clip(cleaned, max_chars)
}

/// Run the isolated one-shot and return a sanitized title. `None` on any
/// failure or timeout, so the caller keeps the literal demand.
pub fn generate_title(
    program: &str,
    cwd: &Path,
    model: &ModelInfo,
    demand: &str,
    max_chars: usize,
) -> Option<String> {
    let mut child = Command::new(program)
        .args(arguments(model, demand))
        .current_dir(cwd)
        .env("PI_SKIP_VERSION_CHECK", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on a worker so a hung Pi can be killed instead of blocking forever.
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stdout.read_to_string(&mut buffer);
        let _ = sender.send(buffer);
    });
    match receiver.recv_timeout(TIMEOUT) {
        Ok(raw) => match child.wait() {
            Ok(status) if status.success() => sanitize(&raw, max_chars),
            _ => None,
        },
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_follow_ups_are_not_substantive() {
        for terse in ["sim", "ok", "pode", "continue", "faz isso", "vai lá", ""] {
            assert!(!is_substantive(terse), "{terse:?} should not rename a session");
        }
    }

    #[test]
    fn real_demands_are_substantive() {
        for demand in [
            "arruma o bug do login",
            "exportar relatório em PDF",
            "refatorar o parser de markdown",
        ] {
            assert!(is_substantive(demand), "{demand:?} should name a session");
        }
    }

    #[test]
    fn normalize_collapses_whitespace_and_clip_bounds_length() {
        assert_eq!(normalize("  um\n\n dois\t três "), "um dois três");
        assert_eq!(clip("  ", 10), None);
        let long = "palavra ".repeat(20);
        assert_eq!(clip(&long, 12).unwrap().chars().count(), 12);
    }

    fn model() -> ModelInfo {
        ModelInfo {
            id: "modelo-x".into(),
            provider: "provedor".into(),
            reasoning: false,
            context_window: 0,
        }
    }

    #[test]
    fn arguments_isolate_pin_the_model_and_carry_only_the_demand() {
        let args = arguments(&model(), "exportar o relatório em PDF");
        assert!(args.contains(&"--print".to_string()));
        assert!(args.contains(&"--no-session".to_string()));
        assert!(args.contains(&"--no-tools".to_string()));
        let provider = args.iter().position(|a| a == "--provider").unwrap();
        assert_eq!(args[provider + 1], "provedor");
        let model_index = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[model_index + 1], "modelo-x");
        let prompt = args.last().unwrap();
        assert!(prompt.starts_with("Escreva um título"));
        assert!(prompt.ends_with("exportar o relatório em PDF"));
    }

    #[test]
    fn sanitize_accepts_a_clean_line_and_strips_decoration() {
        assert_eq!(sanitize("Exportar relatório PDF", 60).as_deref(), Some("Exportar relatório PDF"));
        assert_eq!(sanitize("  \"Corrigir login\"  ", 60).as_deref(), Some("Corrigir login"));
        assert_eq!(sanitize("Título: Corrigir login.", 60).as_deref(), Some("Corrigir login"));
    }

    #[test]
    fn sanitize_rejects_explanations_and_empty_output() {
        assert_eq!(sanitize("", 60), None);
        assert_eq!(sanitize("   \n  ", 60), None);
        assert_eq!(sanitize("Claro, aqui está:\nCorrigir login", 60), None);
    }

    #[test]
    fn sanitize_bounds_the_title_length() {
        let long = "a ".repeat(100);
        assert_eq!(sanitize(&long, 20).unwrap().chars().count(), 20);
    }
}
