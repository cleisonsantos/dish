//! In-memory content cache. Call from a background executor, never the UI thread.
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde_json::Value;

pub const RESULT_LIMIT: usize = 200;
const CACHE_LIMIT: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct Document {
    pub path: Option<PathBuf>,
    pub live: Option<usize>,
    pub title: String,
    /// Live content wins over the persisted file, including streaming messages.
    /// Shared so a query snapshot is not copied again into the worker.
    pub messages: Option<Arc<Vec<String>>>,
}

#[derive(Clone)]
pub struct Hit {
    pub path: Option<PathBuf>,
    pub live: Option<usize>,
    pub title: String,
    pub snippet: String,
    pub message: usize,
    pub occurrences: usize,
}

#[derive(Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub errors: Vec<String>,
    pub limited: bool,
}

struct Cached {
    stamp: (SystemTime, u64),
    messages: Vec<String>,
    bytes: usize,
    oversized: usize,
}

#[derive(Default)]
pub struct Index {
    files: HashMap<PathBuf, Cached>,
    bytes: usize,
}

/// Extract machine-produced content without indexing metadata, credentials in
/// headers, or base64 image payloads. Tool arguments and results are searchable.
pub fn message_text(value: &Value) -> String {
    let mut parts = Vec::new();
    if value["role"] == "bashExecution" {
        for key in ["command", "output"] {
            if let Some(text) = value[key].as_str() {
                parts.push(text.to_owned());
            }
        }
    }
    if let Some(text) = value["content"].as_str() {
        parts.push(text.to_owned());
    }
    if let Some(content) = value["content"].as_array() {
        for block in content {
            for key in ["text", "thinking"] {
                if let Some(text) = block[key].as_str() {
                    parts.push(text.to_owned());
                }
            }
            if block["type"] == "toolCall" {
                if let Some(args) = block.get("arguments") {
                    parts.push(args.to_string());
                }
            }
        }
    }
    parts.join("\n")
}

/// Match offsets in the original UTF-8 text, even when case folding expands
/// a character (for example İ). Bound styling work for pathological inputs.
pub fn match_ranges(text: &str, folded_query: &str) -> Vec<std::ops::Range<usize>> {
    if folded_query.is_empty() {
        return Vec::new();
    }
    if text.is_ascii() {
        return text
            .to_ascii_lowercase()
            .match_indices(folded_query)
            .take(4096)
            .map(|(offset, matched)| offset..offset + matched.len())
            .collect();
    }
    let mut folded = String::new();
    let mut offsets = Vec::new();
    for (offset, ch) in text.char_indices() {
        for lower in ch.to_lowercase() {
            folded.push(lower);
            offsets.extend(std::iter::repeat_n(
                (offset, offset + ch.len_utf8()),
                lower.len_utf8(),
            ));
        }
    }
    folded
        .match_indices(folded_query)
        .take(4096)
        .map(|(offset, matched)| offsets[offset].0..offsets[offset + matched.len() - 1].1)
        .collect()
}

fn snippet(text: &str, needle: &str) -> Option<String> {
    // Keep original Unicode characters; folding can change byte offsets.
    for line in text.lines() {
        let folded = line.to_lowercase();
        let Some(folded_offset) = folded.find(needle) else {
            continue;
        };
        let mut folded_bytes = 0;
        let mut character: usize = 0;
        for ch in line.chars() {
            if folded_bytes >= folded_offset {
                break;
            }
            folded_bytes += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
            character += 1;
        }
        let start = character.saturating_sub(60);
        let mut chars = line.chars().skip(start);
        let snippet: String = chars.by_ref().take(240).collect();
        let truncated = chars.next().is_some();
        return Some(format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            snippet,
            if truncated { "…" } else { "" }
        ));
    }
    None
}

impl Index {
    pub fn search(
        &mut self,
        documents: &[Document],
        query: &str,
        generation: &AtomicU64,
        ticket: u64,
    ) -> Results {
        let needle = query.trim().to_lowercase();
        let mut result = Results::default();
        if needle.is_empty() {
            return result;
        }
        for doc in documents {
            if generation.load(Ordering::Relaxed) != ticket {
                return Results::default();
            }
            let mut oversized = 0;
            let messages = if let Some(messages) = &doc.messages {
                messages
            } else if let Some(path) = &doc.path {
                match self.load(path, generation, ticket) {
                    Ok(true) => match self.files.get(path) {
                        Some(cached) => {
                            oversized = cached.oversized;
                            &cached.messages
                        }
                        None => continue,
                    },
                    Ok(false) => return Results::default(),
                    Err(error) => {
                        result.errors.push(format!("{}: {error}", doc.title));
                        continue;
                    }
                }
            } else {
                continue;
            };
            if oversized > 0 {
                result.errors.push(format!(
                    "{}: {oversized} linha(s) excederam o limite da busca",
                    doc.title
                ));
            }
            for (message, text) in messages.iter().enumerate() {
                if generation.load(Ordering::Relaxed) != ticket {
                    return Results::default();
                }
                if let Some(snippet) = snippet(text, &needle) {
                    if result.hits.len() == RESULT_LIMIT {
                        result.limited = true;
                        return result;
                    }
                    result.hits.push(Hit {
                        path: doc.path.clone(),
                        live: doc.live,
                        title: doc.title.clone(),
                        snippet,
                        message,
                        occurrences: text.to_lowercase().matches(&needle).count(),
                    });
                }
            }
        }
        result
    }

    fn load(
        &mut self,
        path: &PathBuf,
        generation: &AtomicU64,
        ticket: u64,
    ) -> std::io::Result<bool> {
        let metadata = fs::metadata(path)?;
        let stamp = (metadata.modified()?, metadata.len());
        if self
            .files
            .get(path)
            .is_some_and(|cached| cached.stamp == stamp)
        {
            return Ok(true);
        }
        let mut messages = Vec::new();
        let mut bytes = 0;
        let mut oversized = 0;
        let mut reader = BufReader::new(fs::File::open(path)?);
        loop {
            if generation.load(Ordering::Relaxed) != ticket {
                return Ok(false);
            }
            let line = match crate::limits::read_line_limited(
                &mut reader,
                crate::limits::MAX_SEARCH_LINE_BYTES,
                true,
            )? {
                crate::limits::BoundedLine::Line(line) => line,
                crate::limits::BoundedLine::TooLong { bytes } => {
                    oversized += 1;
                    crate::logging::warn(format_args!(
                        "dish: search skipped an oversized line ({} bytes)",
                        bytes
                    ));
                    continue;
                }
                crate::limits::BoundedLine::Eof => break,
            };
            let Ok(entry) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            if entry["type"] != "message" {
                continue;
            }
            let text = message_text(&entry["message"]);
            bytes += text.len();
            messages.push(text);
        }
        if let Some(old) = self.files.remove(path) {
            self.bytes -= old.bytes;
        }
        // Bound retained memory. The current file is admitted even if larger
        // than the budget so it can be searched, then evicted on the next load.
        if self.bytes.saturating_add(bytes) > CACHE_LIMIT {
            self.files.clear();
            self.bytes = 0;
        }
        self.bytes += bytes;
        self.files.insert(
            path.clone(),
            Cached {
                stamp,
                messages,
                bytes,
                oversized,
            },
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_live_unicode_and_caps_results() {
        let docs = vec![Document {
            path: None,
            live: Some(3),
            title: "sessão".into(),
            messages: Some(Arc::new(vec!["ação feita".into(); 250])),
        }];
        let result = Index::default().search(&docs, "AÇÃO", &AtomicU64::new(1), 1);
        assert_eq!(result.hits.len(), RESULT_LIMIT);
        assert!(result.limited);
        assert_eq!(result.hits[0].live, Some(3));
        assert!(result.hits[0].snippet.contains("ação"));
    }

    #[test]
    fn cancellation_and_empty_query_return_no_hits() {
        let docs = vec![Document {
            path: None,
            live: Some(0),
            title: "test".into(),
            messages: Some(Arc::new(vec!["found".into()])),
        }];
        assert!(
            Index::default()
                .search(&docs, "found", &AtomicU64::new(2), 1)
                .hits
                .is_empty()
        );
        assert!(
            Index::default()
                .search(&docs, " ", &AtomicU64::new(1), 1)
                .hits
                .is_empty()
        );
    }

    #[test]
    fn indexes_tools_but_not_images() {
        let message = serde_json::json!({"content": [{"type":"toolCall", "arguments":{"command":"cargo test"}}, {"type":"image", "data":"secret-base64"}, {"type":"text", "text":"passed"}]});
        let text = message_text(&message);
        assert!(text.contains("cargo test"));
        assert!(text.contains("passed"));
        assert!(!text.contains("secret-base64"));
    }

    #[test]
    fn highlight_offsets_preserve_original_utf8_boundaries() {
        let text = "İSTANBUL ação AÇÃO";
        let ranges = match_ranges(text, "ação");
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[0].clone()], "ação");
        assert_eq!(&text[ranges[1].clone()], "AÇÃO");
        assert_eq!(match_ranges(text, "i"), vec![0..2]);
        assert_eq!(match_ranges("aba ABA", "aba"), vec![0..3, 4..7]);
    }

    #[test]
    fn snippet_handles_unicode_case_expansion_and_long_lines() {
        assert_eq!(
            snippet("İSTANBUL ação", "ação"),
            Some("İSTANBUL ação".into())
        );
        let text = format!("{} agulha {}", "x".repeat(10_000), "y".repeat(10_000));
        let found = snippet(&text, "agulha").unwrap();
        assert!(found.contains("agulha"));
        assert!(found.chars().count() <= 242);
    }

    #[test]
    fn unreadable_file_is_reported_without_hiding_other_results() {
        let docs = vec![
            Document {
                path: Some(PathBuf::from(
                    "/nonexistent-dish-search-fixture/session.jsonl",
                )),
                live: None,
                title: "missing".into(),
                messages: None,
            },
            Document {
                path: None,
                live: Some(0),
                title: "live".into(),
                messages: Some(Arc::new(vec!["found".into()])),
            },
        ];
        let result = Index::default().search(&docs, "found", &AtomicU64::new(1), 1);
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    #[ignore = "manual performance measurement; no machine-dependent timing assertion"]
    fn benchmark_large_history() {
        let dir = std::env::temp_dir().join(format!("dish-search-bench-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        let mut data = String::new();
        for i in 0..20_000 {
            data.push_str(&serde_json::json!({"type":"message", "message":{"content":format!("{} marker-{i}", "synthetic history ".repeat(60))}}).to_string());
            data.push('\n');
        }
        fs::write(&path, &data).unwrap();
        let docs = vec![Document {
            path: Some(path),
            live: None,
            title: "large synthetic session".into(),
            messages: None,
        }];
        let mut index = Index::default();
        let generation = AtomicU64::new(1);
        let started = std::time::Instant::now();
        assert_eq!(
            index
                .search(&docs, "marker-19999", &generation, 1)
                .hits
                .len(),
            1
        );
        let cold = started.elapsed();
        let started = std::time::Instant::now();
        assert_eq!(
            index
                .search(&docs, "marker-19998", &generation, 1)
                .hits
                .len(),
            1
        );
        println!(
            "{} bytes / 20,000 messages: cold {:?}, warm {:?}",
            data.len(),
            cold,
            started.elapsed()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unchanged_files_are_cached_and_modified_files_reloaded() {
        let dir = std::env::temp_dir().join(format!("dish-search-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        fs::write(
            &path,
            "{\"type\":\"message\",\"message\":{\"content\":\"old\"}}\n",
        )
        .unwrap();
        let docs = vec![Document {
            path: Some(path.clone()),
            live: None,
            title: "test".into(),
            messages: None,
        }];
        let mut index = Index::default();
        let generation = AtomicU64::new(1);
        assert_eq!(index.search(&docs, "old", &generation, 1).hits.len(), 1);
        assert_eq!(index.files.len(), 1);
        assert!(
            index
                .search(&docs, "missing", &generation, 1)
                .hits
                .is_empty()
        );
        fs::write(
            &path,
            "{\"type\":\"message\",\"message\":{\"content\":\"new content\"}}\n",
        )
        .unwrap();
        assert!(index.search(&docs, "old", &generation, 1).hits.is_empty());
        assert_eq!(index.search(&docs, "new", &generation, 1).hits.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
