//! Read-only Git context. All process and filesystem work belongs off the UI thread.
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(3);
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GitContext {
    #[default]
    Loading,
    Repository(Repository),
    NotRepository,
    Missing,
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub root: PathBuf,
    pub worktree: Option<String>,
    pub branch: String,
    pub changes: usize,
    pub untracked: usize,
    pub conflicts: usize,
    pub ahead_behind: Option<(usize, usize)>,
    pub lines: Option<LineChanges>,
    pub diff_error: Option<String>,
    unborn: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineChanges {
    pub added: usize,
    pub removed: usize,
    pub binary_files: usize,
}

impl Repository {
    pub fn location_label(&self) -> String {
        match &self.worktree {
            Some(worktree) => format!("{} · worktree {worktree}", self.branch),
            None => self.branch.clone(),
        }
    }

    pub fn changes_label(&self) -> String {
        match self.changes {
            0 => "limpo".into(),
            1 => "1 alteração".into(),
            count => format!("{count} alterações"),
        }
    }
}

impl GitContext {
    pub fn summary(&self) -> String {
        match self {
            Self::Loading => "Git · consultando…".into(),
            Self::NotRepository => "Sem repositório Git".into(),
            Self::Missing => "Git não instalado".into(),
            Self::Unavailable(reason) => format!("Git indisponível · {reason}"),
            Self::Repository(repo) => {
                let mut parts = vec![repo.location_label(), repo.changes_label()];
                if let Some(lines) = &repo.lines {
                    if repo.changes > 0 {
                        parts.push(format!("+{} −{}", lines.added, lines.removed));
                    }
                    if lines.binary_files > 0 {
                        parts.push(format!("{} binários", lines.binary_files));
                    }
                }
                if repo.untracked > 0 {
                    parts.push(format!("{} não rastreados", repo.untracked));
                }
                if repo.conflicts > 0 {
                    parts.push(format!("{} conflitos", repo.conflicts));
                }
                if repo.diff_error.is_some() {
                    parts.push("linhas indisponíveis".into());
                }
                if let Some((ahead, behind)) = repo.ahead_behind {
                    if ahead > 0 {
                        parts.push(format!("↑{ahead}"));
                    }
                    if behind > 0 {
                        parts.push(format!("↓{behind}"));
                    }
                }
                parts.join(" · ")
            }
        }
    }
}

fn read_pipe(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0; 8192];
        let mut truncated = false;
        loop {
            let count = pipe.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let remaining = OUTPUT_LIMIT.saturating_sub(kept.len());
            truncated |= count > remaining;
            kept.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        if truncated {
            Err(io::Error::other("saída do Git excedeu o limite"))
        } else {
            Ok(kept)
        }
    })
}

fn run(program: &Path, cwd: &Path, args: &[&str], timeout: Duration) -> io::Result<Output> {
    let mut command = Command::new(program);
    command
        .current_dir(cwd)
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "protocol.allow=never",
        ])
        .args(args)
        // The session cwd, not repository overrides inherited from the shell,
        // determines which checkout the UI describes.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_PREFIX")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::platform::hide_console(&mut command);
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("stdout do Git indisponível"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("stderr do Git indisponível"))?;
    let stdout = read_pipe(stdout);
    let stderr = read_pipe(stderr);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "consulta excedeu o tempo limite",
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    };
    Ok(Output {
        status,
        stdout: stdout
            .join()
            .map_err(|_| io::Error::other("falha ao ler stdout do Git"))??,
        stderr: stderr
            .join()
            .map_err(|_| io::Error::other("falha ao ler stderr do Git"))??,
    })
}

pub fn discover(cwd: &Path) -> GitContext {
    discover_with(Path::new("git"), cwd)
}

fn discover_with(program: &Path, cwd: &Path) -> GitContext {
    match query(program, cwd) {
        Ok(context) => context,
        // A missing cwd is different from a missing executable.
        Err(error) if error.kind() == io::ErrorKind::NotFound && cwd.is_dir() => {
            GitContext::Missing
        }
        Err(error) => GitContext::Unavailable(error.to_string()),
    }
}

fn query(program: &Path, cwd: &Path) -> io::Result<GitContext> {
    let metadata = run(
        program,
        cwd,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
        TIMEOUT,
    )?;
    if !metadata.status.success() {
        if String::from_utf8_lossy(&metadata.stderr).contains("not a git repository") {
            return Ok(GitContext::NotRepository);
        }
        return Err(io::Error::other("não foi possível consultar o repositório"));
    }
    let metadata = String::from_utf8_lossy(&metadata.stdout);
    let paths: Vec<_> = metadata.lines().collect();
    if paths.len() != 3 {
        return Err(io::Error::other("resposta de diretório do Git inválida"));
    }
    let root = PathBuf::from(paths[0]);
    let git_dir = PathBuf::from(paths[1]);
    let common_dir = PathBuf::from(paths[2]);
    let worktree = (git_dir != common_dir).then(|| {
        root.file_name()
            .unwrap_or(root.as_os_str())
            .to_string_lossy()
            .into_owned()
    });
    let status = run(
        program,
        cwd,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
            "-z",
        ],
        TIMEOUT,
    )?;
    if !status.status.success() {
        return Err(io::Error::other(
            "não foi possível consultar o estado do Git",
        ));
    }
    let mut repo = parse_status(&status.stdout)?;
    repo.root = root;
    repo.worktree = worktree;
    let lines = if repo.changes == repo.untracked {
        Ok(LineChanges::default())
    } else {
        line_changes(program, cwd, repo.unborn)
    };
    match lines {
        Ok(lines) => repo.lines = Some(lines),
        Err(error) => repo.diff_error = Some(error.to_string()),
    }
    Ok(GitContext::Repository(repo))
}

/// A single diff against HEAD covers staged + unstaged changes without double
/// counting. An unborn branch uses Git's empty tree (hash-object without -w).
fn line_changes(program: &Path, cwd: &Path, unborn: bool) -> io::Result<LineChanges> {
    let base = if unborn {
        let empty = run(
            program,
            cwd,
            &["hash-object", "-t", "tree", "--stdin"],
            TIMEOUT,
        )?;
        if !empty.status.success() {
            return Err(io::Error::other(
                "não foi possível consultar a árvore vazia",
            ));
        }
        String::from_utf8_lossy(&empty.stdout).trim().to_owned()
    } else {
        "HEAD".into()
    };
    let output = run(
        program,
        cwd,
        &[
            "-c",
            "diff.renameLimit=200",
            "diff",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            &base,
            "--",
        ],
        TIMEOUT,
    )?;
    if !output.status.success() {
        return Err(io::Error::other(
            "não foi possível contar as linhas do diff",
        ));
    }
    parse_numstat(&output.stdout)
}

fn parse_numstat(bytes: &[u8]) -> io::Result<LineChanges> {
    let mut changes = LineChanges::default();
    let mut records = bytes.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let added = fields
            .next()
            .ok_or_else(|| io::Error::other("numstat inválido"))?;
        let removed = fields
            .next()
            .ok_or_else(|| io::Error::other("numstat inválido"))?;
        let name = fields
            .next()
            .ok_or_else(|| io::Error::other("caminho ausente no numstat"))?;
        if added == b"-" && removed == b"-" {
            changes.binary_files += 1;
        } else {
            let count = |field: &[u8]| -> io::Result<usize> {
                std::str::from_utf8(field)
                    .ok()
                    .and_then(|text| text.parse().ok())
                    .ok_or_else(|| io::Error::other("contagem de linhas inválida"))
            };
            changes.added = changes
                .added
                .checked_add(count(added)?)
                .ok_or_else(|| io::Error::other("contagem excedeu o limite"))?;
            changes.removed = changes
                .removed
                .checked_add(count(removed)?)
                .ok_or_else(|| io::Error::other("contagem excedeu o limite"))?;
        }
        // With -z a rename has an empty name followed by old and new paths.
        if name.is_empty() {
            let old = records.next().filter(|path| !path.is_empty());
            let new = records.next().filter(|path| !path.is_empty());
            if old.is_none() || new.is_none() {
                return Err(io::Error::other("rename incompleto no numstat"));
            }
        }
    }
    Ok(changes)
}

fn parse_status(bytes: &[u8]) -> io::Result<Repository> {
    let mut branch = None;
    let mut oid = None;
    let mut ahead_behind = None;
    let mut changes = 0;
    let mut untracked = 0;
    let mut conflicts = 0;
    let mut records = bytes.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            branch = Some(String::from_utf8_lossy(value).into_owned());
        } else if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            oid = Some(String::from_utf8_lossy(value).into_owned());
        } else if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let text = String::from_utf8_lossy(value);
            if let Some((ahead, behind)) = text.split_once(' ') {
                ahead_behind = ahead
                    .strip_prefix('+')
                    .and_then(|n| n.parse().ok())
                    .zip(behind.strip_prefix('-').and_then(|n| n.parse().ok()));
            }
        } else if matches!(record.first(), Some(b'1' | b'2' | b'u' | b'?')) {
            changes += 1;
            if record.first() == Some(&b'?') {
                untracked += 1;
            }
            if record.first() == Some(&b'u') {
                conflicts += 1;
            }
            if record.first() == Some(&b'2') {
                records.next();
            } // Original name after rename, even if it looks like a record.
        }
    }
    let mut branch = branch.ok_or_else(|| io::Error::other("branch ausente na resposta do Git"))?;
    let unborn = oid.as_deref() == Some("(initial)");
    if branch == "(detached)" {
        let oid = oid
            .filter(|oid| oid != "(initial)")
            .ok_or_else(|| io::Error::other("commit ausente em detached HEAD"))?;
        branch = format!("HEAD {}", oid.chars().take(8).collect::<String>());
    }
    Ok(Repository {
        root: PathBuf::new(),
        worktree: None,
        branch,
        changes,
        untracked,
        conflicts,
        ahead_behind,
        lines: None,
        diff_error: None,
        unborn,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dish-git-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().into()
    }

    #[test]
    fn parses_unborn_detached_divergence_and_renames_without_counting_paths() {
        let unborn =
            parse_status(b"# branch.oid (initial)\0# branch.head main\0? new file\0").unwrap();
        assert_eq!(unborn.branch, "main");
        assert_eq!(unborn.changes, 1);
        let repo = parse_status(b"# branch.oid abcdef1234567890\0# branch.head (detached)\0# branch.ab +2 -3\x001 M. changed\x002 R. renamed\0? old name\0u UU conflict\0? newline\nfile\0").unwrap();
        assert_eq!(repo.branch, "HEAD abcdef12");
        assert_eq!(repo.changes, 4);
        assert_eq!(repo.ahead_behind, Some((2, 3)));
        assert!(parse_status(b"# branch.head (detached)\0").is_err());
    }

    #[test]
    fn summaries_are_compact_and_non_repo_states_are_explicit() {
        let repo = GitContext::Repository(Repository {
            root: "/synthetic/repo".into(),
            worktree: Some("task".into()),
            branch: "main".into(),
            changes: 3,
            untracked: 0,
            conflicts: 0,
            ahead_behind: Some((1, 2)),
            lines: Some(LineChanges {
                added: 12,
                removed: 4,
                binary_files: 0,
            }),
            diff_error: None,
            unborn: false,
        });
        assert_eq!(
            repo.summary(),
            "main · worktree task · 3 alterações · +12 −4 · ↑1 · ↓2"
        );
        assert_eq!(GitContext::NotRepository.summary(), "Sem repositório Git");
        assert_eq!(GitContext::Missing.summary(), "Git não instalado");
    }

    #[test]
    fn discovers_real_repo_subfolder_worktree_and_detached_head_without_writing_index() {
        let temp = Temp::new();
        git(&temp.0, &["init", "-b", "main"]);
        std::fs::write(temp.0.join("tracked"), "original").unwrap();
        git(&temp.0, &["add", "tracked"]);
        git(
            &temp.0,
            &[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        let index = std::fs::read(temp.0.join(".git/index")).unwrap();
        std::fs::create_dir(temp.0.join("sub")).unwrap();
        let GitContext::Repository(clean) = discover(&temp.0.join("sub")) else {
            panic!("repository not found")
        };
        assert_eq!(clean.branch, "main");
        assert_eq!(clean.changes, 0);
        assert_eq!(clean.worktree, None);
        assert_eq!(std::fs::read(temp.0.join(".git/index")).unwrap(), index);
        assert!(!temp.0.join(".git/index.lock").exists());
        std::fs::write(temp.0.join("tracked"), "changed").unwrap();
        std::fs::write(temp.0.join("untracked"), "new").unwrap();
        let GitContext::Repository(dirty) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(dirty.changes, 2);
        assert_eq!(std::fs::read(temp.0.join(".git/index")).unwrap(), index);
        let linked = temp.0.join("linked");
        git(
            &temp.0,
            &["worktree", "add", "-b", "task", linked.to_str().unwrap()],
        );
        let GitContext::Repository(worktree) = discover(&linked) else {
            panic!("worktree not found")
        };
        assert_eq!(worktree.branch, "task");
        assert_eq!(worktree.worktree.as_deref(), Some("linked"));
        git(&linked, &["checkout", "--detach"]);
        let GitContext::Repository(detached) = discover(&linked) else {
            panic!("detached repository not found")
        };
        assert!(detached.branch.starts_with("HEAD "));
    }

    #[test]
    fn divergence_uses_only_local_upstream_refs() {
        let temp = Temp::new();
        git(&temp.0, &["init", "-b", "main"]);
        std::fs::write(temp.0.join("tracked"), "base").unwrap();
        git(&temp.0, &["add", "tracked"]);
        let commit = [
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ];
        git(&temp.0, &commit);
        git(&temp.0, &["branch", "upstream"]);
        git(&temp.0, &["branch", "--set-upstream-to=upstream"]);
        std::fs::write(temp.0.join("tracked"), "ahead").unwrap();
        git(&temp.0, &["add", "tracked"]);
        git(&temp.0, &commit);
        let GitContext::Repository(ahead) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(ahead.ahead_behind, Some((1, 0)));
        git(&temp.0, &["checkout", "upstream"]);
        std::fs::write(temp.0.join("tracked"), "behind").unwrap();
        git(&temp.0, &["add", "tracked"]);
        git(&temp.0, &commit);
        git(&temp.0, &["checkout", "main"]);
        let GitContext::Repository(diverged) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(diverged.ahead_behind, Some((1, 1)));
    }

    #[test]
    fn numstat_handles_binary_renames_and_unusual_names() {
        let changes =
            parse_numstat(b"12\t3\tfile\tname\nline\0-\t-\tbinary\x000\t0\t\0old name\0new name\0")
                .unwrap();
        assert_eq!(
            changes,
            LineChanges {
                added: 12,
                removed: 3,
                binary_files: 1
            }
        );
        assert!(parse_numstat(b"2\t1\t\0old\0").is_err());
        assert!(parse_numstat(b"not-a-number\t1\tfile\0").is_err());
    }

    #[test]
    fn line_counts_are_net_head_changes_not_staged_plus_unstaged_sum() {
        let temp = Temp::new();
        git(&temp.0, &["init", "-b", "main"]);
        std::fs::write(temp.0.join("tracked"), "one\ntwo\nthree\n").unwrap();
        git(&temp.0, &["add", "tracked"]);
        git(
            &temp.0,
            &[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        std::fs::write(temp.0.join("tracked"), "one\nstaged\ntwo\nthree\n").unwrap();
        git(&temp.0, &["add", "tracked"]);
        std::fs::write(temp.0.join("tracked"), "one\nreplacement\n").unwrap();
        std::fs::write(temp.0.join("new-file"), "untracked\nlines\nnot\ncounted\n").unwrap();
        let index = std::fs::read(temp.0.join(".git/index")).unwrap();
        let GitContext::Repository(repo) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(
            repo.lines,
            Some(LineChanges {
                added: 1,
                removed: 2,
                binary_files: 0
            })
        );
        assert_eq!(repo.changes, 2);
        assert_eq!(repo.untracked, 1);
        assert_eq!(repo.diff_error, None);
        assert_eq!(std::fs::read(temp.0.join(".git/index")).unwrap(), index);
    }

    #[test]
    fn unborn_line_counts_compare_worktree_to_empty_tree_without_writing_it() {
        let temp = Temp::new();
        git(&temp.0, &["init", "-b", "main"]);
        std::fs::write(temp.0.join("tracked"), "one\ntwo\nthree\n").unwrap();
        git(&temp.0, &["add", "tracked"]);
        std::fs::write(temp.0.join("tracked"), "two\n").unwrap();
        let index = std::fs::read(temp.0.join(".git/index")).unwrap();
        let empty = git(&temp.0, &["hash-object", "-t", "tree", "--stdin"]);
        let tree_path = temp
            .0
            .join(".git/objects")
            .join(&empty[..2])
            .join(&empty[2..]);
        assert!(!tree_path.exists());
        let GitContext::Repository(repo) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(
            repo.lines,
            Some(LineChanges {
                added: 1,
                removed: 0,
                binary_files: 0
            })
        );
        assert!(repo.unborn);
        assert!(!tree_path.exists());
        assert_eq!(std::fs::read(temp.0.join(".git/index")).unwrap(), index);
    }

    #[test]
    fn pure_rename_does_not_inflate_line_counts() {
        let temp = Temp::new();
        git(&temp.0, &["init", "-b", "main"]);
        std::fs::write(temp.0.join("old name"), "one\ntwo\nthree\n").unwrap();
        git(&temp.0, &["add", "old name"]);
        git(
            &temp.0,
            &[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        git(&temp.0, &["mv", "old name", "new name"]);
        let GitContext::Repository(repo) = discover(&temp.0) else {
            panic!("repository not found")
        };
        assert_eq!(repo.changes, 1);
        assert_eq!(repo.lines, Some(LineChanges::default()));
    }

    #[test]
    fn handles_missing_git_and_non_repo() {
        let temp = Temp::new();
        assert_eq!(discover(&temp.0), GitContext::NotRepository);
        assert_eq!(
            discover_with(&temp.0.join("absent-git"), &temp.0),
            GitContext::Missing
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminates_hanging_command() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new();
        let script = temp.0.join("fake-git");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        let error = run(&script, &temp.0, &[], Duration::from_millis(40)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
