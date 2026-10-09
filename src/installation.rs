//! Pi onboarding. No installer is executed until the user explicitly requests it.
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const PACKAGE: &str = "@earendil-works/pi-coding-agent";
static SELECTED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn selected() -> Option<PathBuf> {
    SELECTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(PathBuf::from)
}

pub fn remember(program: &str) {
    *SELECTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(program.to_owned());
    let mut preferences = crate::preferences::load();
    preferences.pi_executable = Some(program.into());
    crate::preferences::save(preferences);
}

/// Dish's data directory (`$XDG_DATA_HOME/dish`, default `~/.local/share/dish`).
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .map(|p| p.join("dish"))
}

pub fn prefix() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("pi"))
}

fn executable(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("pi.cmd")
    } else {
        prefix.join("bin/pi")
    }
}

pub fn program() -> String {
    resolve(
        std::env::var("DISH_PI_BIN").ok(),
        crate::preferences::load().pi_executable,
        find_on_path("pi").or_else(find_installed),
        prefix().map(|p| executable(&p)).filter(|p| p.is_file()),
    )
}

fn resolve(
    override_program: Option<String>,
    selected: Option<PathBuf>,
    on_path: Option<PathBuf>,
    private: Option<PathBuf>,
) -> String {
    override_program.unwrap_or_else(|| {
        selected
            .or(on_path)
            .or(private)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pi".into())
    })
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let path = dir.join(if cfg!(windows) {
            format!("{name}.cmd")
        } else {
            name.into()
        });
        path.is_file().then_some(path)
    })
}

fn find_installed() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    [
        home.join(".local/bin/pi"),
        home.join(".npm-global/bin/pi"),
        PathBuf::from("/usr/local/bin/pi"),
        PathBuf::from("/opt/homebrew/bin/pi"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

// Fixed script: no project path, credentials or user input is interpolated.
const OFFICIAL_SCRIPT: &str = "curl -fsSL https://pi.dev/install.sh | sh; result=$?; printf '\\nInstalador finalizado (status %s). Volte ao Dish e clique em Tentar novamente.\\nPressione Enter para fechar.\\n' \"$result\"; read answer";

fn launch_terminal(program: &Path, flag: &str) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args([flag, "/bin/sh", "-c", OFFICIAL_SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = retry_text_busy(|| command.spawn(), Duration::from_millis(250))
        .context("Não foi possível abrir o terminal")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn launch_official_installer() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("Neste sistema, copie o comando oficial e execute-o em um terminal.");
    }
    for (name, flag) in [
        ("x-terminal-emulator", "-e"),
        ("gnome-terminal", "--"),
        ("konsole", "-e"),
        ("xfce4-terminal", "-x"),
        ("kitty", "--"),
        ("alacritty", "-e"),
        ("xterm", "-e"),
    ] {
        if let Some(path) = find_on_path(name) {
            if launch_terminal(&path, flag).is_ok() {
                return Ok(());
            }
        }
    }
    bail!("Nenhum terminal compatível foi encontrado. Copie o comando oficial, execute-o em um terminal e tente novamente.");
}

// An executable just written by another thread/process can briefly return
// ETXTBSY on Unix. Retry only that transient error, with a bounded deadline.
fn retry_text_busy<T>(mut attempt: impl FnMut() -> std::io::Result<T>, timeout: Duration) -> std::io::Result<T> {
    let deadline = Instant::now() + timeout.min(Duration::from_millis(250));
    loop {
        match attempt() {
            Err(error) if is_text_busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

fn is_text_busy(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    { error.raw_os_error() == Some(libc::ETXTBSY) }
    #[cfg(not(unix))]
    { let _ = error; false }
}

// Drain both pipes while waiting, retaining only bounded version output.
fn run(mut command: Command, timeout: Duration) -> Result<String> {
    use std::io::Read;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let deadline = Instant::now() + timeout;
    let mut child = retry_text_busy(|| command.spawn(), timeout)
        .context("Não foi possível executar o comando")?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0; 1024];
        while let Ok(count) = stdout.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let remaining = 4096usize.saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        kept
    });
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("O comando terminou com erro ({status}).");
            }
            let bytes = reader
                .join()
                .map_err(|_| anyhow!("Falha ao ler a versão"))?;
            return Ok(String::from_utf8_lossy(&bytes).trim().to_string());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("O comando excedeu o tempo limite.");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn validate(program: &str) -> Result<String> {
    let mut command = Command::new(program);
    command.arg("--version");
    let version = run(command, Duration::from_secs(15))?;
    if !version.lines().any(|line| parse_version(line).is_some()) {
        bail!("O executável não retornou uma versão reconhecível do Pi.");
    }
    Ok(version)
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word.trim_start_matches('v').split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.split('-').next()?.parse().ok()?,
        ))
    })
}

pub fn prerequisites(node: &str, npm: &str) -> Result<()> {
    let mut command = Command::new(node);
    command.arg("--version");
    let version = run(command, Duration::from_secs(15))?;
    if parse_version(&version).is_none_or(|v| v < (22, 19, 0)) {
        bail!("É necessário Node.js 22.19 ou superior. Use as instruções oficiais.");
    }
    let mut command = Command::new(npm);
    command.arg("--version");
    run(command, Duration::from_secs(15)).context("É necessário npm para instalar o Pi.")?;
    Ok(())
}

pub fn install(npm: &str, destination: &Path) -> Result<String> {
    let mut command = Command::new(npm);
    command
        .args([
            "install",
            "-g",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--prefix",
        ])
        .arg(destination)
        .arg(PACKAGE);
    // Never show arbitrary npm output: it can contain private registry credentials.
    run(command, Duration::from_secs(600))?;
    let path = executable(destination).to_string_lossy().into_owned();
    validate(&path)?;
    Ok(path)
}

pub fn manual_command() -> &'static str {
    if cfg!(windows) {
        "powershell -c \"irm https://pi.dev/install.ps1 | iex\""
    } else {
        "curl -fsSL https://pi.dev/install.sh | sh"
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dish-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn script(&self, name: &str, script: &str) -> String {
            let path = self.0.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            path.to_string_lossy().into_owned()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn transient_exec_busy_is_retried_but_other_errors_are_not() {
        let mut attempts = 0;
        let result = retry_text_busy(|| {
            attempts += 1;
            if attempts < 3 { Err(std::io::Error::from_raw_os_error(libc::ETXTBSY)) } else { Ok(42) }
        }, Duration::from_secs(1));
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);
        let mut attempts = 0;
        let result: std::io::Result<()> = retry_text_busy(|| {
            attempts += 1;
            Err(std::io::Error::from_raw_os_error(libc::ENOENT))
        }, Duration::from_secs(1));
        assert!(result.is_err());
        assert_eq!(attempts, 1);
        let result: std::io::Result<()> = retry_text_busy(|| Err(std::io::Error::from_raw_os_error(libc::ETXTBSY)), Duration::ZERO);
        assert!(result.is_err());
    }

    #[test]
    fn detects_missing_failed_and_invalid_executables() {
        let f = Fixture::new();
        assert!(validate(&f.0.join("missing").to_string_lossy()).is_err());
        assert!(validate(&f.script("failed", "exit 1")).is_err());
        assert!(validate(&f.script("invalid", "printf 'not pi'")).is_err());
        assert_eq!(
            validate(&f.script("pi", "printf '0.84.4'")).unwrap(),
            "0.84.4"
        );
    }
    #[test]
    fn checks_node_and_npm_prerequisites() {
        let f = Fixture::new();
        let npm = f.script("npm", "printf '10.0.0'");
        assert!(prerequisites(&f.script("old-node", "printf 'v22.18.0'"), &npm).is_err());
        assert!(prerequisites(&f.script("node", "printf 'v22.19.0'"), &npm).is_ok());
        assert!(prerequisites(
            &f.script("new-node", "printf 'v24.0.0'"),
            "missing-dish-npm"
        )
        .is_err());
    }
    #[test]
    fn installs_with_fake_npm_and_validates_result() {
        let f = Fixture::new();
        let destination = f.0.join("private");
        let npm = f.script(
            "npm",
            r#"test "$1" = install && test "$3" = --ignore-scripts || exit 2
mkdir -p "$7/bin"
printf '#!/bin/sh\nprintf "0.84.4"\n' > "$7/bin/pi"
chmod +x "$7/bin/pi""#,
        );
        assert_eq!(
            install(&npm, &destination).unwrap(),
            destination.join("bin/pi").to_string_lossy()
        );
        assert!(install(&f.script("fail-npm", "exit 1"), &destination).is_err());
    }
    #[test]
    fn override_wins_and_private_install_is_found_without_path_changes() {
        assert_eq!(
            resolve(
                Some("/override".into()),
                Some("/chosen".into()),
                Some("/path/pi".into()),
                Some("/private/pi".into())
            ),
            "/override"
        );
        assert_eq!(
            resolve(
                None,
                Some("/chosen".into()),
                Some("/path/pi".into()),
                Some("/private/pi".into())
            ),
            "/chosen"
        );
        assert_eq!(
            resolve(None, None, None, Some("/private/pi".into())),
            "/private/pi"
        );
        assert_eq!(resolve(None, None, None, None), "pi");
    }
    #[test]
    fn rejects_installer_success_without_a_working_pi() {
        let f = Fixture::new();
        assert!(install(&f.script("npm", "exit 0"), &f.0.join("private")).is_err());
    }
    #[test]
    fn hanging_command_is_terminated() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exec sleep 10"]);
        assert!(run(command, Duration::from_millis(50)).is_err());
    }
    #[test]
    fn external_terminal_receives_fixed_installer_arguments() {
        let f = Fixture::new();
        let output = f.0.join("arguments");
        let terminal = f.script(
            "terminal",
            &format!("printf '%s\\n' \"$@\" > '{}'", output.display()),
        );
        launch_terminal(Path::new(&terminal), "-e").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(arguments) = std::fs::read_to_string(&output) {
                if arguments == format!("-e\n/bin/sh\n-c\n{OFFICIAL_SCRIPT}\n") {
                    break;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(launch_terminal(&f.0.join("missing"), "-e").is_err());
    }
}
