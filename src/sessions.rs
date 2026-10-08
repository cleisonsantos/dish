use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub title: String,
    pub modified: std::time::SystemTime,
}

pub fn expand_path(value: &str, cwd: &Path) -> PathBuf {
    let path = if value == "~" || value.starts_with("~/") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    };
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

pub fn roots(cwd: &Path, flags: &[String]) -> Vec<PathBuf> {
    let agent = std::env::var("PI_CODING_AGENT_DIR")
        .map(|p| expand_path(&p, cwd))
        .unwrap_or_else(|_| expand_path("~/.pi/agent", cwd));
    let mut roots = vec![agent.join("sessions")];
    for settings in [agent.join("settings.json"), cwd.join(".pi/settings.json")] {
        if let Ok(content) = fs::read_to_string(settings) {
            if let Ok(value) = serde_json::from_str::<Value>(&content) {
                if let Some(dir) = value["sessionDir"].as_str() {
                    roots.push(expand_path(dir, cwd));
                }
            }
        }
    }
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_SESSION_DIR") {
        roots.push(expand_path(&dir, cwd));
    }
    for (index, flag) in flags.iter().enumerate() {
        if flag == "--session-dir" {
            if let Some(dir) = flags.get(index + 1) {
                roots.push(expand_path(dir, cwd));
            }
        } else if let Some(dir) = flag.strip_prefix("--session-dir=") {
            roots.push(expand_path(dir, cwd));
        }
    }
    roots
}

#[cfg(test)]
pub fn discover(roots: &[PathBuf]) -> Vec<SessionInfo> {
    discover_cached(roots, &[])
}

pub fn discover_cached(roots: &[PathBuf], previous: &[SessionInfo]) -> Vec<SessionInfo> {
    let mut paths = std::collections::BTreeSet::new();
    for root in roots {
        collect_files(root, 0, &mut paths);
    }
    let cached: std::collections::BTreeMap<_, _> =
        previous.iter().map(|info| (&info.path, info)).collect();
    let mut sessions: Vec<_> = paths
        .iter()
        .filter_map(|path| {
            if let Some(info) = cached.get(path) {
                if fs::metadata(path).ok()?.modified().ok()? == info.modified {
                    return Some((*info).clone());
                }
            }
            read_session(path)
        })
        .collect();
    sessions.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    sessions
}

fn collect_files(dir: &Path, depth: usize, paths: &mut std::collections::BTreeSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") && path.is_file() {
            paths.insert(path.canonicalize().unwrap_or(path));
        } else if depth == 0 && path.is_dir() {
            collect_files(&path, depth + 1, paths);
        }
    }
}

fn read_session(path: &Path) -> Option<SessionInfo> {
    let file = fs::File::open(path).ok()?;
    let modified = file.metadata().ok()?.modified().ok()?;
    let mut lines = BufReader::new(file).lines();
    let header: Value = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }
    let cwd = PathBuf::from(header.get("cwd")?.as_str()?);
    if !cwd.is_absolute() {
        return None;
    }
    let mut name = None;
    let mut first = None;
    for line in lines.map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value["type"] == "session_info" {
            name = value["name"].as_str().map(str::to_owned);
        } else if first.is_none()
            && value["type"] == "message"
            && value["message"]["role"] == "user"
        {
            let content = &value["message"]["content"];
            let text = content.as_str().map(str::to_owned).unwrap_or_else(|| {
                content
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| part["text"].as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default()
            });
            first = Some(
                text.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect::<String>(),
            );
        }
    }
    let title = name
        .filter(|name| !name.trim().is_empty())
        .or(first.filter(|text| !text.is_empty()))
        .unwrap_or_else(|| "Untitled session".into());
    Some(SessionInfo {
        path: path.to_owned(),
        cwd,
        title,
        modified,
    })
}

/// Keep tool/model configuration, but never inherit the initial session selector.
pub fn launch_flags(flags: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    let mut skip = false;
    for flag in flags {
        if skip {
            skip = false;
            continue;
        }
        if ["--session", "--session-id", "--name", "-n"].contains(&flag.as_str()) {
            skip = true;
        } else if [
            "--continue",
            "-c",
            "--resume",
            "-r",
            "--fork",
            "--no-session",
        ]
        .contains(&flag.as_str())
            || ["--session=", "--session-id=", "--name="]
                .iter()
                .any(|prefix| flag.starts_with(prefix))
        {
            continue;
        } else {
            output.push(flag.clone());
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_and_groups_by_header_not_directory_name() {
        let root = std::env::temp_dir().join(format!("dish-sessions-{}", std::process::id()));
        fs::create_dir_all(root.join("encoded")).unwrap();
        fs::write(root.join("encoded/a.jsonl"), "{\"type\":\"session\",\"cwd\":\"/project/a\"}\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"First prompt\"}]}}\n{\"type\":\"session_info\",\"name\":\"Named thread\"}\n").unwrap();
        fs::write(root.join("bad.jsonl"), "invalid").unwrap();
        let result = discover(&[root.clone(), root.clone()]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].cwd, PathBuf::from("/project/a"));
        assert_eq!(result[0].title, "Named thread");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strips_original_session_selection() {
        let flags = [
            "--continue",
            "--session",
            "old.jsonl",
            "--model",
            "test",
            "--session-dir=/tmp/custom",
        ]
        .map(str::to_owned);
        assert_eq!(
            launch_flags(&flags),
            vec!["--model", "test", "--session-dir=/tmp/custom"]
        );
    }

    #[test]
    fn distinguishes_same_named_projects_and_caches_unchanged_files() {
        let root = std::env::temp_dir().join(format!("dish-projects-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("a.jsonl"),
            "{\"type\":\"session\",\"cwd\":\"/one/app\"}\n",
        )
        .unwrap();
        fs::write(
            root.join("b.jsonl"),
            "{\"type\":\"session\",\"cwd\":\"/two/app\"}\n",
        )
        .unwrap();
        let mut previous = discover(std::slice::from_ref(&root));
        assert_eq!(previous.len(), 2);
        assert_ne!(previous[0].cwd, previous[1].cwd);
        // A sentinel proves unchanged metadata is reused rather than reparsed.
        previous[0].title = "cached".into();
        let cached = discover_cached(std::slice::from_ref(&root), &previous);
        assert_eq!(cached[0].title, "cached");
        fs::remove_file(&cached[0].path).unwrap();
        assert_eq!(
            discover_cached(std::slice::from_ref(&root), &cached).len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
        assert!(discover(&[PathBuf::from("/nonexistent/dish-test-sessions")]).is_empty());
    }
}
