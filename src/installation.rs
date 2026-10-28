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
/// Raiz de dados: XDG no Unix, `%LOCALAPPDATA%` no Windows.
fn data_root(xdg: Option<PathBuf>, local_app_data: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg.or(local_app_data).or_else(|| home.map(|home| home.join(".local/share")))
}

pub fn data_dir() -> Option<PathBuf> {
    let local_app_data = if cfg!(windows) { env_path("LOCALAPPDATA") } else { None };
    data_root(env_path("XDG_DATA_HOME"), local_app_data, env_path("HOME")).map(|root| root.join("dish"))
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

/// O que o adaptador de autenticação precisa para achar o pacote do Pi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiLocation {
    /// Executável que o Dish usa para conversar com o Pi.
    pub program: Option<PathBuf>,
    /// Diretório do pacote npm. Para um caminho explícito que não contenha o
    /// pacote, o caminho derivado é passado mesmo assim: o adaptador então
    /// falha fechado em vez de usar outra instalação em silêncio.
    pub package: Option<PathBuf>,
}

/// Executável do Pi e pacote correspondente, na mesma ordem de `program`.
pub fn pi_location() -> PiLocation {
    let mut location = locate(
        env_path("DISH_PI_BIN"),
        crate::preferences::load().pi_executable,
        find_on_path("pi").or_else(find_installed),
        prefix().map(|p| executable(&p)).filter(|p| p.is_file()),
    );
    // `DISH_PI_PACKAGE` é autoridade do adaptador; um `--package` derivado aqui
    // poderia sobrepor o caminho que a pessoa escolheu.
    if env_path("DISH_PI_PACKAGE").is_some() {
        location.package = None;
    }
    location
}

fn locate(
    override_program: Option<PathBuf>,
    selected: Option<PathBuf>,
    on_path: Option<PathBuf>,
    private: Option<PathBuf>,
) -> PiLocation {
    let (program, explicit) = choose(override_program, selected, on_path, private);
    let package = program
        .as_deref()
        .and_then(|program| resolve_package(program, explicit));
    PiLocation { program, package }
}

/// Resolve o executável e diz se ele foi escolhido explicitamente.
///
/// A escolha explícita vence a descoberta; sem escolha, o `PATH` vence o
/// prefixo privado do Dish, como em `program`.
fn choose(
    override_program: Option<PathBuf>,
    selected: Option<PathBuf>,
    on_path: Option<PathBuf>,
    private: Option<PathBuf>,
) -> (Option<PathBuf>, bool) {
    if let Some(program) = override_program {
        return (Some(program), true);
    }
    if let Some(program) = selected {
        return (Some(program), true);
    }
    (on_path.or(private), false)
}

fn resolve(
    override_program: Option<String>,
    selected: Option<PathBuf>,
    on_path: Option<PathBuf>,
    private: Option<PathBuf>,
) -> String {
    choose(
        override_program.map(PathBuf::from),
        selected,
        on_path,
        private,
    )
    .0
    .map(|p| p.to_string_lossy().into_owned())
    .unwrap_or_else(|| "pi".into())
}

/// Diretórios onde o pacote do Pi pode estar, a partir do executável.
fn package_candidates(program: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    // `bin/pi` de um npm global costuma ser um link para dentro do pacote.
    if let Ok(resolved) = std::fs::canonicalize(program) {
        if let Some(package) = resolved
            .ancestors()
            .find(|ancestor| ancestor.ends_with(PACKAGE))
        {
            candidates.push(package.to_path_buf());
        }
    }
    if let Some(parent) = program.parent() {
        // `<prefixo>/bin/pi`, `<prefixo>/pi.cmd` e o prefixo local do npm.
        // Em `<prefixo>/bin/pi` o prefixo é o pai do `bin`, não o próprio `bin`.
        let roots: Vec<&Path> = if parent.file_name().and_then(|name| name.to_str()) == Some("bin")
        {
            parent.parent().into_iter().collect()
        } else {
            [Some(parent), parent.parent()]
                .into_iter()
                .flatten()
                .collect()
        };
        for root in roots {
            candidates.push(root.join("node_modules").join(PACKAGE));
            candidates.push(root.join("lib").join("node_modules").join(PACKAGE));
        }
    }
    candidates
}

fn resolve_package(program: &Path, explicit: bool) -> Option<PathBuf> {
    let candidates = package_candidates(program);
    if let Some(package) = candidates
        .iter()
        .find(|package| package.join("dist").join("index.js").is_file())
    {
        return Some(package.clone());
    }
    // Caminho explícito sem pacote não cai para outra instalação: o primeiro
    // candidato vai ao adaptador e vira `package_invalid`.
    if explicit {
        candidates.into_iter().next()
    } else {
        None
    }
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

/// Diretórios onde o Pi costuma ficar, além do `PATH`. Inclui o destino do
/// instalador oficial no Windows (`%USERPROFILE%\.pi\agent\bin`), que ele
/// adiciona ao `PATH` do usuário — mas só para processos abertos depois disso,
/// e é por isso que "já instalei, verificar" precisa procurar aqui também.
fn pi_candidates(home: Option<PathBuf>, local_app_data: Option<PathBuf>, app_data: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = home {
        for relative in [".local/bin/pi", ".npm-global/bin/pi", ".pi/agent/bin/pi.cmd", ".pi/agent/bin/pi.exe", ".pi/agent/bin/pi"] {
            candidates.push(home.join(relative));
        }
    }
    if let Some(app_data) = app_data {
        candidates.push(app_data.join("npm/pi.cmd"));
    }
    if let Some(local) = local_app_data {
        candidates.push(local.join("Programs/pi/pi.cmd"));
        candidates.push(local.join("pi/pi.cmd"));
    }
    candidates.extend([
        PathBuf::from("/usr/local/bin/pi"),
        PathBuf::from("/opt/homebrew/bin/pi"),
    ]);
    candidates
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn find_installed() -> Option<PathBuf> {
    // No Windows `HOME` não existe; `USERPROFILE` é o equivalente.
    let home = env_path("HOME").or_else(|| env_path("USERPROFILE"));
    pi_candidates(home, env_path("LOCALAPPDATA"), env_path("APPDATA"))
        .into_iter()
        .find(|path| path.is_file())
}

// Fixed script: no project path, credentials or user input is interpolated.
const OFFICIAL_SCRIPT: &str = r#"temp_file=$(mktemp /tmp/dish-pi-installer.XXXXXX)
result=$?
if [ "$result" -eq 0 ]; then
    trap 'rm -f "$temp_file"' EXIT
    curl -fsSL --proto '=https' --tlsv1.2 -o "$temp_file" https://pi.dev/install.sh
    result=$?
    if [ "$result" -eq 0 ]; then
        sh "$temp_file"
        result=$?
    fi
    rm -f "$temp_file"
    trap - EXIT
fi
printf '\nInstalador finalizado (status %s). Volte ao Dish e clique em Tentar novamente.\nPressione Enter para fechar.\n' "$result"
read answer"#;

// O Dish não autentica: abre o Pi para a pessoa usar `/login`. Sem caminho,
// sem credencial e sem entrada do usuário interpolados no script.
const PI_LOGIN_SCRIPT: &str = "pi || printf '\\nNão foi possível iniciar o Pi. Verifique se ele está no PATH.\\n'; printf '\\nNo Pi, use /login para conectar um provedor. Pressione Enter para fechar.\\n'; read answer";

fn launch_terminal(program: &Path, flag: &str, script: &str) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args([flag, "/bin/sh", "-c", script])
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

/// Executa um script nosso no primeiro terminal compatível encontrado.
pub fn launch_script_in_terminal(script: &str) -> Result<()> {
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
            if launch_terminal(&path, flag, script).is_ok() {
                return Ok(());
            }
        }
    }
    bail!("Nenhum terminal compatível foi encontrado.");
}

/// Abre o Pi num terminal para a pessoa entrar com `/login`.
///
/// Enquanto o Dish não escreve credencial (#69, fase 3), este é o caminho
/// honesto: o Pi conduz o OAuth e guarda o resultado, e o Dish só relê o estado.
pub fn launch_pi_login() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("Neste sistema, abra o Pi em um terminal e use /login.");
    }
    launch_script_in_terminal(PI_LOGIN_SCRIPT)
        .context("Não encontrei um terminal. Abra o Pi e use /login.")
}

pub fn launch_official_installer() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("Neste sistema, copie o comando oficial e execute-o em um terminal.");
    }
    launch_script_in_terminal(OFFICIAL_SCRIPT).context(
        "Nenhum terminal compatível foi encontrado. Copie o comando oficial, execute-o em um terminal e tente novamente.",
    )
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

// Keep the bounded runner shared with the other utility processes; only the
// spawn attempt is retried for the transient ETXTBSY case.  npm can be
// verbose, so allow a larger bounded buffer even though most callers only
// read a version line.
fn run(command: Command, timeout: Duration) -> Result<String> {
    let output = crate::process::run_with_spawner(
        command,
        timeout,
        8 * 1024 * 1024,
        None,
        None,
        |command| retry_text_busy(|| command.spawn(), timeout),
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::TimedOut {
            anyhow!("O comando excedeu o tempo limite.")
        } else {
            anyhow!("Não foi possível executar o comando: {error}")
        }
    })?;
    if !output.status.success() {
        bail!("O comando terminou com erro ({}).", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
        include_str!("../scripts/install-pi-windows.ps1")
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
    fn data_root_follows_the_platform_convention() {
        let xdg = Some(PathBuf::from("/xdg"));
        let local = Some(PathBuf::from("C:/Users/dish/AppData/Local"));
        let home = Some(PathBuf::from("/home/dish"));
        assert_eq!(data_root(xdg.clone(), local.clone(), home.clone()), xdg);
        assert_eq!(data_root(None, local.clone(), home.clone()), local);
        assert_eq!(data_root(None, None, home), Some(PathBuf::from("/home/dish/.local/share")));
        assert_eq!(data_root(None, None, None), None);
    }

    #[test]
    fn windows_install_locations_are_searched_without_path() {
        let home = PathBuf::from("C:/Users/dish");
        let candidates = pi_candidates(
            Some(home.clone()),
            Some(PathBuf::from("C:/Users/dish/AppData/Local")),
            Some(PathBuf::from("C:/Users/dish/AppData/Roaming")),
        );
        // Destino do instalador oficial do Pi no Windows.
        assert!(candidates.contains(&home.join(".pi/agent/bin/pi.cmd")), "{candidates:?}");
        assert!(candidates.iter().any(|path| path.ends_with("npm/pi.cmd")));
        assert!(candidates.iter().any(|path| path.ends_with("Programs/pi/pi.cmd")));
        // Sem perfil de usuário ainda restam os caminhos de sistema.
        let system = pi_candidates(None, None, None);
        assert!(system.contains(&PathBuf::from("/usr/local/bin/pi")));
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
    fn explicit_program_without_a_package_still_names_the_candidate() {
        let f = Fixture::new();
        let program = f.0.join("bin/pi");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let location = locate(Some(program.clone()), None, None, None);
        assert_eq!(location.program, Some(program));
        // O adaptador recebe um caminho de pacote mesmo quando ele não existe:
        // assim a falha é `package_invalid`, nunca outro Pi em silêncio.
        assert_eq!(
            location.package,
            Some(f.0.join("node_modules").join(PACKAGE))
        );
    }

    #[test]
    fn discovered_program_passes_an_existing_package() {
        let f = Fixture::new();
        let prefix = f.0.join("dish/pi");
        let package = prefix.join("lib/node_modules").join(PACKAGE);
        std::fs::create_dir_all(package.join("dist")).unwrap();
        std::fs::write(package.join("dist/index.js"), "export {};").unwrap();
        let program = prefix.join("bin/pi");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        let location = locate(None, None, Some(program.clone()), None);
        assert_eq!(location.program, Some(program));
        assert_eq!(location.package, Some(package));
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
    fn official_installer_checks_download_before_execution() {
        assert!(!OFFICIAL_SCRIPT.contains("| sh"));
        assert!(OFFICIAL_SCRIPT.contains("curl -fsSL --proto '=https' --tlsv1.2 -o \"$temp_file\" https://pi.dev/install.sh\n    result=$?\n    if [ \"$result\" -eq 0 ]; then\n        sh \"$temp_file\""));
        assert!(OFFICIAL_SCRIPT.contains("trap 'rm -f \"$temp_file\"' EXIT"));
        // The fixed command contains only shell variables, not interpolated Rust input.
        assert!(!OFFICIAL_SCRIPT.contains("{}"));
    }

    #[test]
    fn external_terminal_receives_fixed_installer_arguments() {
        let f = Fixture::new();
        let output = f.0.join("arguments");
        let terminal = f.script(
            "terminal",
            &format!("printf '%s\\n' \"$@\" > '{}'", output.display()),
        );
        launch_terminal(Path::new(&terminal), "-e", OFFICIAL_SCRIPT).unwrap();
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
        assert!(launch_terminal(&f.0.join("missing"), "-e", OFFICIAL_SCRIPT).is_err());
    }
}
