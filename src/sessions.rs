use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub title: String,
    /// O título veio de um `session_info` explícito (`/name`, `--name`), e não
    /// de uma demanda do usuário. Nome explícito nunca é substituído.
    pub named: bool,
    pub modified: std::time::SystemTime,
}

/// A session file waiting in Dish's trash.
#[derive(Clone, Debug)]
pub struct TrashedSession {
    pub trashed: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TrashMeta {
    original: PathBuf,
    cwd: PathBuf,
    title: String,
    deleted_at: u64,
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

/// Dish's trash for deleted session files. It lives outside the discovery
/// roots, so a trashed session never shows up in the rail again.
pub fn trash_root() -> Option<PathBuf> {
    crate::installation::data_dir().map(|dir| dir.join("trash"))
}

/// Moves a session file to Dish's trash.
pub fn trash(info: &SessionInfo) -> std::io::Result<TrashedSession> {
    let root = trash_root()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no data directory available"))?;
    trash_into(&root, info)
}

/// `trash` with an explicit destination, so tests never touch the real data dir.
pub fn trash_into(root: &Path, info: &SessionInfo) -> std::io::Result<TrashedSession> {
    if info.path.extension().is_none_or(|ext| ext != "jsonl") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a session file",
        ));
    }
    fs::create_dir_all(root)?;
    let (trashed, meta) = loop {
        let stem = unique_trash_name();
        let trashed = root.join(format!("{stem}.jsonl"));
        let meta = root.join(format!("{stem}.json"));
        if !trashed.exists() && !meta.exists() {
            break (trashed, meta);
        }
    };
    let metadata = TrashMeta {
        original: info.path.clone(),
        cwd: info.cwd.clone(),
        title: info.title.clone(),
        deleted_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0),
    };
    // Serialize before touching the session: a path that cannot be represented
    // must leave the original file in place, not strand it without metadata.
    let encoded = serde_json::to_vec_pretty(&metadata).map_err(io::Error::other)?;
    move_file_no_clobber(&info.path, &trashed)?;
    let metadata_error = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&meta)
    {
        Ok(mut file) => match file.write_all(&encoded).and_then(|_| file.sync_all()) {
            Ok(()) => None,
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&meta);
                Some(error)
            }
        },
        Err(error) => Some(error),
    };
    if let Some(error) = metadata_error {
        // Never leave a session in the trash without its metadata.  If the
        // original path was recreated in the meantime, keep both files and
        // report where the session can be recovered from.
        if let Err(rollback) = move_file_no_clobber(&trashed, &info.path) {
            crate::logging::warn(format_args!(
                "dish: cannot restore session after metadata failure: {rollback}; session kept at {}",
                trashed.display()
            ));
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; não foi possível devolver a sessão ao caminho original ({rollback}); ela continua em {}",
                    trashed.display()
                ),
            ));
        }
        return Err(error);
    }
    Ok(TrashedSession { trashed })
}

/// Moves a trashed session back to its original path and drops the metadata.
///
/// The original path may have been recreated while the session was in the
/// trash.  Restoration is no-clobber: an existing file is left untouched and
/// the session remains recoverable in the trash.
pub fn restore(trashed: &Path, original: &Path) -> std::io::Result<()> {
    if let Some(parent) = original.parent() {
        fs::create_dir_all(parent)?;
    }
    move_file_no_clobber(trashed, original)?;
    let meta = trashed.with_extension("json");
    if let Err(error) = fs::remove_file(&meta) {
        if error.kind() != io::ErrorKind::NotFound {
            // The session is already back in place.  A stale metadata file only
            // leaves an artifact in the trash; it must not turn a successful
            // restoration into a failure.
            crate::logging::warn(format_args!(
                "dish: cannot remove session trash metadata {}: {error}",
                meta.display()
            ));
        }
    }
    Ok(())
}

fn unique_trash_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!(
        "{millis}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Moves a file without replacing an existing destination.
///
/// `rename` replaces the destination on Unix and Windows, which is unsafe for
/// a restore: another process may have recreated the original session while it
/// was in the trash.  A hard link is the efficient same-filesystem primitive;
/// when it is unavailable (for example across a `--session-dir` mount), copy
/// into a destination opened with `create_new`.
fn move_file_no_clobber(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => {
            if let Err(error) = fs::remove_file(from) {
                let _ = fs::remove_file(to);
                return Err(error);
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(_) => copy_file_no_clobber(from, to),
    }
}

fn copy_file_no_clobber(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut source = fs::File::open(from)?;
    let permissions = source.metadata()?.permissions();
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    let copied = io::copy(&mut source, &mut destination)
        .and_then(|_| destination.set_permissions(permissions))
        .and_then(|_| destination.sync_all());
    if let Err(error) = copied {
        drop(destination);
        let _ = fs::remove_file(to);
        return Err(error);
    }
    drop(destination);
    if let Err(error) = fs::remove_file(from) {
        let _ = fs::remove_file(to);
        return Err(error);
    }
    Ok(())
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
    let mut last = None;
    let mut last_substantive = None;
    for line in lines.map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value["type"] == "session_info" {
            name = value["name"].as_str().map(str::to_owned);
        } else if value["type"] == "message" && value["message"]["role"] == "user" {
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
            let Some(title) = crate::demand::clip(&text, 80) else {
                continue;
            };
            if crate::demand::is_substantive(&text) {
                last_substantive = Some(title.clone());
            }
            last = Some(title);
        }
    }
    let named = name.as_deref().is_some_and(|name| !name.trim().is_empty());
    let title = name
        .filter(|name| !name.trim().is_empty())
        .or(last_substantive)
        .or(last)
        .unwrap_or_else(|| "Untitled session".into());
    Some(SessionInfo {
        path: path.to_owned(),
        cwd,
        named,
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
        assert!(result[0].named, "session_info names are explicit");
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
    fn trashes_and_restores_a_session() {
        let root = std::env::temp_dir().join(format!("dish-trash-{}", std::process::id()));
        let sessions = root.join("sessions");
        let trash = root.join("trash");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("a.jsonl");
        fs::write(&path, "{\"type\":\"session\",\"cwd\":\"/project/a\"}\n").unwrap();
        let info = SessionInfo {
            path: path.clone(),
            cwd: PathBuf::from("/project/a"),
            title: "Named thread".into(),
            named: true,
            modified: fs::metadata(&path).unwrap().modified().unwrap(),
        };

        let entry = trash_into(&trash, &info).unwrap();
        assert!(!path.exists());
        assert!(entry.trashed.exists());
        let meta = entry.trashed.with_extension("json");
        assert!(meta.exists());

        restore(&entry.trashed, &path).unwrap();
        assert!(path.exists());
        assert!(!entry.trashed.exists());
        assert!(!meta.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restoring_never_overwrites_a_recreated_session() {
        let root = std::env::temp_dir().join(format!("dish-trash-conflict-{}", std::process::id()));
        let sessions = root.join("sessions");
        let trash = root.join("trash");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("a.jsonl");
        fs::write(&path, "old session").unwrap();
        let info = SessionInfo {
            path: path.clone(),
            cwd: root.clone(),
            title: "Conflict".into(),
            named: true,
            modified: fs::metadata(&path).unwrap().modified().unwrap(),
        };

        let entry = trash_into(&trash, &info).unwrap();
        fs::write(&path, "new session").unwrap();

        let error = restore(&entry.trashed, &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new session");
        assert_eq!(fs::read_to_string(&entry.trashed).unwrap(), "old session");
        assert!(
            entry.trashed.with_extension("json").exists(),
            "trash metadata must survive a conflict"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unrepresentable_path_is_not_moved_without_metadata() {
        use std::os::unix::ffi::OsStringExt as _;

        let root = std::env::temp_dir().join(format!("dish-trash-utf8-{}", std::process::id()));
        let sessions = root.join("sessions");
        let trash = root.join("trash");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join(std::ffi::OsString::from_vec(b"bad-\xff.jsonl".to_vec()));
        fs::write(&path, "session").unwrap();
        let info = SessionInfo {
            path: path.clone(),
            cwd: root.clone(),
            title: "Unrepresentable".into(),
            named: true,
            modified: fs::metadata(&path).unwrap().modified().unwrap(),
        };

        assert!(trash_into(&trash, &info).is_err());
        assert!(path.exists(), "the session must stay in place");
        assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_clobber_move_preserves_both_files_on_conflict() {
        let root = std::env::temp_dir().join(format!("dish-trash-move-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.jsonl");
        let destination = root.join("destination.jsonl");
        fs::write(&source, "source").unwrap();
        fs::write(&destination, "destination").unwrap();

        let error = move_file_no_clobber(&source, &destination).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&source).unwrap(), "source");
        assert_eq!(fs::read_to_string(&destination).unwrap(), "destination");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_trash_a_non_session_file() {
        let root = std::env::temp_dir().join(format!("dish-trash-guard-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("notes.txt");
        fs::write(&path, "keep me").unwrap();
        let info = SessionInfo {
            path: path.clone(),
            cwd: root.clone(),
            title: "notes".into(),
            named: false,
            modified: fs::metadata(&path).unwrap().modified().unwrap(),
        };

        assert!(trash_into(&root.join("trash"), &info).is_err());
        assert!(path.exists());
        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn unnamed_session_title_follows_the_latest_demand() {
        let root = std::env::temp_dir().join(format!("dish-demand-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("a.jsonl"),
            concat!(
                "{\"type\":\"session\",\"cwd\":\"/project/a\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"arruma o bug do login\"}}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"feito\"}]}}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"continue\"}}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"agora exporta o relatório em PDF\"}}\n",
            ),
        )
        .unwrap();
        let result = discover(std::slice::from_ref(&root));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "agora exporta o relatório em PDF");
        assert!(!result[0].named, "a derived demand is not an explicit name");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_short_follow_up_does_not_rename_the_session() {
        let root = std::env::temp_dir().join(format!("dish-demand-short-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("a.jsonl"),
            concat!(
                "{\"type\":\"session\",\"cwd\":\"/project/a\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"refatorar o parser de markdown\"}}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"continue\"}}\n",
            ),
        )
        .unwrap();
        let result = discover(std::slice::from_ref(&root));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "refatorar o parser de markdown");
        fs::remove_dir_all(root).unwrap();
    }
}
