//! Diagnóstico que nunca entra em pânico: stderr, `dish.log` e `crash.log`.
//!
//! No Windows, builds de release usam o subsistema GUI: quando o Dish é aberto
//! pelo Explorer ou pelo menu Iniciar não existe stderr válido, e `eprintln!`
//! entraria em pânico ao falhar a escrita. Estes avisos são de diagnóstico —
//! falhar ao escrevê-los nunca pode derrubar o aplicativo.
//!
//! O launcher descarta stderr (issue #147), então `warn` também anexa em
//! `dish.log` e o hook de pânico grava `crash.log`. Tudo passa pela redação
//! antes de tocar o disco; ver `docs/diagnostics.md`.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Acima deste tamanho `dish.log` roda para `dish.log.1`.
const LOG_FILE_MAX_BYTES: u64 = 256 * 1024;
/// Teto total de `crash.log`, somando o que já estava gravado.
const CRASH_LOG_MAX_BYTES: u64 = 64 * 1024;
const LOG_FILE_NAME: &str = "dish.log";
const CRASH_FILE_NAME: &str = "crash.log";
const MAX_REDACTED_BYTES: usize = 2048;
const TRUNCATED_SUFFIX: &str = " …[truncado]";
const REDACTED: &str = "<redigido>";
const REDACTED_SESSION: &str = "<sessão>";
const REDACTED_AGENT_DIR: &str = "<pi-agent>";

/// Diretório resolvido por `init`; `None` quando não há lugar gravável.
static LOG_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
/// `DISH_LOG_FILE=0` desliga só o `dish.log`; o `crash.log` continua.
static FILE_LOG: OnceLock<bool> = OnceLock::new();
static APPEND_LOCK: Mutex<()> = Mutex::new(());
static IN_PANIC: AtomicBool = AtomicBool::new(false);

/// Diretório de estado do Dish usado pelos diagnósticos.
///
/// `XDG_STATE_HOME` manda; sem ele, o Windows usa `%LOCALAPPDATA%\dish\logs` e
/// os demais sistemas `$HOME/.local/share/dish/logs`.
pub fn log_dir() -> Option<PathBuf> {
    let local_app_data = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        None
    };
    log_dir_from(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        local_app_data,
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// Resolve o diretório a partir de valores já lidos do ambiente (testável).
///
/// Valores vazios contam como ausentes: `XDG_STATE_HOME=""` não vira `/dish`.
fn log_dir_from(
    state: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let usable = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
    if let Some(state) = usable(state) {
        return Some(state.join("dish"));
    }
    if let Some(local_app_data) = usable(local_app_data) {
        return Some(local_app_data.join("dish").join("logs"));
    }
    usable(home).map(|home| home.join(".local").join("share").join("dish").join("logs"))
}

/// Prepara o diretório de diagnóstico, roda o `crash.log` e instala o hook.
///
/// Deve ser a primeira coisa feita em `main`. Erros de criação são ignorados:
/// sem diretório, os avisos continuam apenas em stderr.
pub fn init() {
    let enabled = !std::env::var("DISH_LOG_FILE").is_ok_and(|value| value == "0");
    FILE_LOG.get_or_init(|| enabled);
    let dir = log_dir().filter(|dir| create_private_dir(dir));
    LOG_DIR.get_or_init(|| dir.clone());
    if let Some(dir) = dir.as_deref() {
        // Uma rotação por execução: copie o arquivo anterior antes de reabrir.
        rotate_crash_log(dir);
    }
    install_panic_hook();
}

/// Caminho de `crash.log`, quando `init` achou um diretório utilizável.
pub fn crash_log_path() -> Option<PathBuf> {
    LOG_DIR
        .get()
        .and_then(Option::as_ref)
        .map(|dir| dir.join(CRASH_FILE_NAME))
}

/// Caminho de `dish.log`, quando `init` achou um diretório utilizável.
pub fn log_file_path() -> Option<PathBuf> {
    LOG_DIR
        .get()
        .and_then(Option::as_ref)
        .map(|dir| dir.join(LOG_FILE_NAME))
}

/// Escreve um aviso de diagnóstico, ignorando erros de escrita.
pub fn warn(args: std::fmt::Arguments<'_>) {
    let text = format!("{args}");
    let _ = writeln!(std::io::stderr(), "{text}");
    if !FILE_LOG.get().copied().unwrap_or(false) {
        return;
    }
    let Some(path) = log_file_path() else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let line = format!("{} {}\n", timestamp(), redact(&text));
    // Nunca em pânico: um mutex envenenado só custa a serialização da linha.
    let _guard = APPEND_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    append_rotating(dir, &line, LOG_FILE_MAX_BYTES);
}

/// Anexa uma linha, rodando `dish.log` para `dish.log.1` ao passar do teto.
fn append_rotating(dir: &Path, line: &str, max: u64) {
    let path = dir.join(LOG_FILE_NAME);
    let rotated = dir.join(format!("{LOG_FILE_NAME}.1"));
    let is_full = fs::metadata(&path).is_ok_and(|meta| meta.len() >= max);
    if is_full {
        // `rename` não sobrescreve no Windows; remover antes é obrigatório.
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(&path, &rotated);
    }
    if let Ok(mut file) = open_private_append(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Cria o diretório com permissão 0o700 no Unix. `false` quando não dá.
#[cfg(unix)]
fn create_private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .is_ok()
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> bool {
    fs::create_dir_all(dir).is_ok()
}

/// Abre em append+create com permissão 0o600 no Unix.
#[cfg(unix)]
fn open_private_append(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private_append(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new().append(true).create(true).open(path)
}

/// Roda `crash.log` para `crash.log.1`, sobrescrevendo o anterior.
fn rotate_crash_log(dir: &Path) {
    let path = dir.join(CRASH_FILE_NAME);
    if !path.is_file() {
        return;
    }
    let rotated = dir.join(format!("{CRASH_FILE_NAME}.1"));
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(&path, &rotated);
}

/// Instala o hook de pânico que grava o relatório redigido.
///
/// O hook padrão não é chamado: ele imprimiria a mensagem sem redação.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        // Pânico dentro do hook abortaria o processo sem relatório: sai cedo.
        if IN_PANIC.swap(true, Ordering::SeqCst) {
            return;
        }
        let report = crash_report(info);
        let _ = std::io::stderr().write_all(report.as_bytes());
        append_crash_log(&report);
        // Um pânico posterior, em outra thread, ainda merece relatório.
        IN_PANIC.store(false, Ordering::SeqCst);
    }));
}

/// Monta o relatório de pânico, já redigido e com quebra de linha final.
fn crash_report(info: &std::panic::PanicHookInfo<'_>) -> String {
    let home = std::env::var("HOME").ok();
    let pi_dir = std::env::var("PI_CODING_AGENT_DIR").ok();
    let payload = match info.payload().downcast_ref::<&str>() {
        Some(text) => (*text).to_owned(),
        None => match info.payload().downcast_ref::<String>() {
            Some(text) => text.clone(),
            None => "<payload não textual>".to_owned(),
        },
    };
    let message = redact_with(&payload, home.as_deref(), pi_dir.as_deref());
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "desconhecido".to_owned());
    // Em builds locais o arquivo pode ser um caminho absoluto em $HOME/.cargo;
    // a localização passa pela mesma redação da mensagem.
    let location = redact_unbounded(&location, home.as_deref(), pi_dir.as_deref());
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("<sem nome>");
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let backtrace = redact_lines(&backtrace, home.as_deref(), pi_dir.as_deref());
    // O backtrace não passa pelo corte de 2048 bytes, mas nunca fica maior que
    // o orçamento total do arquivo.
    let backtrace = fit_to_budget(&backtrace, 0, CRASH_LOG_MAX_BYTES);
    let mut report = format!(
        "Dish crash report\n\
         data: {}\n\
         versão: {}\n\
         thread: {thread}\n\
         mensagem: {message}\n\
         local: {location}\n\
         símbolos: build sem debuginfo — o backtrace mostra funções, sem arquivo/linha\n\
         backtrace:\n\
         {backtrace}",
        timestamp(),
        crate::version::display_version(),
    );
    if !report.ends_with('\n') {
        report.push('\n');
    }
    report
}

/// Anexa o relatório em `crash.log`, respeitando o teto de 64 KiB no total.
fn append_crash_log(report: &str) {
    let Some(path) = crash_log_path() else {
        return;
    };
    let existing = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let slice = fit_to_budget(report, existing, CRASH_LOG_MAX_BYTES);
    if slice.is_empty() {
        return;
    }
    if let Ok(mut file) = open_private_append(&path) {
        let _ = file.write_all(slice.as_bytes());
    }
}

/// Fatia do relatório que ainda cabe no orçamento, em limite de caractere.
///
/// Devolve `""` quando o arquivo já gastou o teto.
fn fit_to_budget(report: &str, existing: u64, limit: u64) -> &str {
    let room = limit.saturating_sub(existing).min(report.len() as u64) as usize;
    if room >= report.len() {
        return report;
    }
    let mut end = room;
    while end > 0 && !report.is_char_boundary(end) {
        end -= 1;
    }
    &report[..end]
}

/// Timestamp UTC em RFC3339, com precisão de segundos.
fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Redige um texto de diagnóstico com o ambiente atual e corta em 2048 bytes.
pub fn redact(text: &str) -> String {
    let home = std::env::var("HOME").ok();
    let pi_dir = std::env::var("PI_CODING_AGENT_DIR").ok();
    redact_with(text, home.as_deref(), pi_dir.as_deref())
}

/// Redige com ambiente injetado; corte final em 2048 bytes.
fn redact_with(text: &str, home: Option<&str>, pi_dir: Option<&str>) -> String {
    truncate_bytes(
        &redact_unbounded(text, home, pi_dir),
        MAX_REDACTED_BYTES,
        TRUNCATED_SUFFIX,
    )
}

/// Redige sem cortar tamanho (backtraces passam por aqui, linha a linha).
fn redact_unbounded(text: &str, home: Option<&str>, pi_dir: Option<&str>) -> String {
    let mut text = text.to_owned();
    if let Some(dir) = pi_dir.filter(|dir| !dir.is_empty()) {
        text = text.replace(dir, REDACTED_AGENT_DIR);
    }
    if let Some(home) = home.filter(|home| home.len() > 1) {
        text = text.replace(home, "~");
    }
    redact_words(&text)
}

/// Redige cada linha sem aplicar o corte de 2048 bytes.
fn redact_lines(text: &str, home: Option<&str>, pi_dir: Option<&str>) -> String {
    text.lines()
        .map(|line| redact_unbounded(line, home, pi_dir))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Redige palavra a palavra, preservando espaços e a pontuação de separação.
fn redact_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for character in text.chars() {
        if is_word_separator(character) {
            out.push_str(&redact_word(&word));
            word.clear();
            out.push(character);
        } else {
            word.push(character);
        }
    }
    out.push_str(&redact_word(&word));
    out
}

fn is_word_separator(character: char) -> bool {
    character.is_whitespace() || matches!(character, '"' | '\'' | ',')
}

/// Regras de uma palavra: caminho de sessão, chave solta e `nome=valor`.
fn redact_word(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    if word.contains("/sessions/") || word.ends_with(".jsonl") {
        return REDACTED_SESSION.to_owned();
    }
    let lower = word.to_ascii_lowercase();
    if lower.starts_with("sk-") {
        return REDACTED.to_owned();
    }
    let mut marker: Option<(usize, usize)> = None;
    for name in ["token=", "key=", "secret=", "password=", "authorization:"] {
        if let Some(position) = lower.find(name) {
            let better = match marker {
                Some((best, _)) => position < best,
                None => true,
            };
            if better {
                marker = Some((position, name.len()));
            }
        }
    }
    let Some((position, length)) = marker else {
        return word.to_owned();
    };
    let prefix = &word[..position];
    let keeps_name = prefix
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-');
    if keeps_name {
        // Mantém "nome=" para o leitor reconhecer o campo; o valor nunca sobra.
        format!("{}{}", &word[..position + length], REDACTED)
    } else {
        REDACTED.to_owned()
    }
}

/// Corta em limite de caractere e sinaliza o corte com um sufixo.
fn truncate_bytes(text: &str, max: usize, suffix: &str) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &text[..end])
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn warning_never_panics() {
        super::warn(format_args!("dish: teste {}", 1));
    }

    #[test]
    fn log_dir_prefers_xdg_state_home() {
        let dir = super::log_dir_from(
            Some(std::path::PathBuf::from("/tmp/state")),
            Some(std::path::PathBuf::from("/tmp/local")),
            Some(std::path::PathBuf::from("/home/u")),
        );
        assert_eq!(dir, Some(std::path::PathBuf::from("/tmp/state/dish")));
    }

    #[test]
    fn log_dir_uses_local_app_data_without_state_home() {
        let dir = super::log_dir_from(
            None,
            Some(std::path::PathBuf::from("/tmp/local")),
            Some(std::path::PathBuf::from("/home/u")),
        );
        assert_eq!(dir, Some(std::path::PathBuf::from("/tmp/local/dish/logs")));
    }

    #[test]
    fn log_dir_falls_back_to_home() {
        let dir = super::log_dir_from(None, None, Some(std::path::PathBuf::from("/home/u")));
        assert_eq!(
            dir,
            Some(std::path::PathBuf::from("/home/u/.local/share/dish/logs"))
        );
    }

    #[test]
    fn log_dir_is_none_for_empty_values() {
        assert_eq!(
            super::log_dir_from(
                Some(std::path::PathBuf::new()),
                Some(std::path::PathBuf::new()),
                Some(std::path::PathBuf::new()),
            ),
            None
        );
        assert_eq!(super::log_dir_from(None, None, None), None);
    }

    #[test]
    fn redaction_hides_secrets_paths_and_sessions() {
        let home = Some("/home/u");
        let pi_dir = Some("/tmp/agent");
        assert_eq!(
            super::redact_with("chave sk-live-ABC", home, pi_dir),
            "chave <redigido>"
        );
        let token = super::redact_with("token=xyz", home, pi_dir);
        assert!(token.contains("token=<redigido>"), "{token}");
        assert!(!token.contains("xyz"), "{token}");
        assert_eq!(super::redact_with("/home/u/x", home, pi_dir), "~/x");
        assert_eq!(
            super::redact_with("/tmp/agent/sessions/x/alpha.jsonl", home, pi_dir),
            "<sessão>"
        );
        assert_eq!(
            super::redact_with("veja /tmp/agent/sessions/test", home, pi_dir),
            "veja <sessão>"
        );
        assert_eq!(
            super::redact_with("password=hunter2", home, pi_dir),
            "password=<redigido>"
        );
    }

    #[test]
    fn redaction_truncates_long_text() {
        let long = "a".repeat(5000);
        let redacted = super::redact_with(&long, None, None);
        assert!(redacted.ends_with(super::TRUNCATED_SUFFIX));
        assert!(redacted.len() <= super::MAX_REDACTED_BYTES + super::TRUNCATED_SUFFIX.len());
    }

    #[test]
    fn backtrace_lines_are_not_truncated() {
        let line = "a".repeat(4096);
        assert_eq!(super::redact_lines(&line, None, None).len(), 4096);
    }

    #[test]
    fn crash_budget_respects_existing_bytes() {
        let report = "relatório";
        assert_eq!(super::fit_to_budget(report, 0, 64), report);
        assert_eq!(super::fit_to_budget(report, 64, 64), "");
        assert_eq!(super::fit_to_budget(report, 128, 64), "");
        assert_eq!(super::fit_to_budget("abcdef", 4, 6), "ab");
        // `á` ocupa dois bytes: o corte em 1 byte recua para o limite válido.
        assert_eq!(super::fit_to_budget("ábc", 0, 1), "");
        assert_eq!(super::fit_to_budget("ábc", 0, 2), "á");
    }

    #[test]
    fn log_file_rotates_at_size_limit() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("dish-log-test-{}-{}", std::process::id(), nanos));
        fs::create_dir_all(&root).unwrap();
        let max = 32;
        let rotated = root.join(format!("{}.1", super::LOG_FILE_NAME));

        super::append_rotating(&root, "linha\n", max);
        assert_eq!(
            fs::read_to_string(root.join(super::LOG_FILE_NAME)).unwrap(),
            "linha\n"
        );
        assert!(!rotated.exists());

        fs::write(root.join(super::LOG_FILE_NAME), "y".repeat(max as usize)).unwrap();
        super::append_rotating(&root, "nova\n", max);
        assert_eq!(fs::read_to_string(&rotated).unwrap().len(), max as usize);
        assert_eq!(
            fs::read_to_string(root.join(super::LOG_FILE_NAME)).unwrap(),
            "nova\n"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
