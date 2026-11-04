//! Provedores locais e compatíveis com OpenAI, gravados no `models.json` do Pi.
//!
//! O contrato de segurança desta fronteira é curto: a chave de API só pode
//! aparecer no cabeçalho `Authorization` enviado ao endpoint escolhido pelo
//! usuário. Ela nunca entra em mensagens de erro, `Debug`, logs ou nomes de
//! arquivo. Mensagens de erro são construídas *só* a partir de `FetchError` e
//! nunca incluem o corpo da resposta, o texto do erro do `ureq`, cabeçalhos ou
//! a própria chave.
//!
//! O arquivo `models.json` pode conter chaves, então é sempre gravado com modo
//! 0600 (inclusive o backup). Campos que este Dish não conhece são preservados:
//! o Pi é dono do formato, e uma atualização futura não pode apagar configuração
//! do usuário. Toda chamada HTTP aqui é bloqueante e deve rodar numa thread de
//! fundo, como as demais consultas de provedores.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use url::Url;

const SCHEMA: &str = "https://pi.dev/schemas/models.schema.json";
const RESPONSE_LIMIT: u64 = 1024 * 1024;
const MAX_MODELS: usize = 500;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const SAVE_TEMP_ATTEMPTS: u32 = 8;
const SAVE_BACKUP: &str = "models.json.dish-backup";
const SAVE_FILE: &str = "models.json";

/// Tempo curto: descoberta é uma tentativa de conveniência, não um bloqueio.
pub const DETECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// Tempo maior: validar é uma ação explícita da tela de configurações.
pub const VALIDATE_TIMEOUT: Duration = Duration::from_secs(8);

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Dialeto que o Pi usa para falar com o endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    OpenAiCompletions,
    OpenAiResponses,
}

impl Api {
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAiCompletions => "openai-completions",
            Self::OpenAiResponses => "openai-responses",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompletions => "Chat Completions",
            Self::OpenAiResponses => "Responses",
        }
    }
}

/// Serviços locais que o Dish sabe descobrir sem configuração.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalService {
    Ollama,
    LmStudio,
}

impl LocalService {
    pub const ALL: [LocalService; 2] = [Self::Ollama, Self::LmStudio];

    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
        }
    }

    pub fn default_root(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::LmStudio => "http://127.0.0.1:1234",
        }
    }

    /// URL que entra em `models.json` (`/v1` é o dialeto OpenAI dos dois).
    pub fn base_url(self, root: &str) -> String {
        format!("{}/v1", root.trim_end_matches('/'))
    }

    /// Chave literal exigida pelo Pi para o modelo existir, ignorada pelo serviço.
    pub fn dummy_key(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lm-studio",
        }
    }
}

/// Resultado de procurar um serviço local. `NotFound` não é erro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    Found {
        service: LocalService,
        models: Vec<String>,
    },
    NotFound {
        service: LocalService,
    },
    Failed {
        service: LocalService,
        message: String,
    },
}

/// O que a tela pretende salvar em `models.json`.
pub struct Endpoint {
    pub provider: String,
    pub base_url: String,
    pub api: Api,
    pub key: Option<String>,
    pub models: Vec<String>,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("api", &self.api)
            .field("key", &self.key.as_deref().map(|_| "<oculta>"))
            .field("models", &self.models)
            .finish()
    }
}

/// Erros de rede já normalizados. O corpo e o texto original do `ureq` não
/// atravessam esta enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    Unreachable,
    Status(u16),
    TooLarge,
    BadJson,
    BadShape,
    Cancelled,
    InvalidUrl,
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            Self::Unreachable => {
                "Não foi possível alcançar o endpoint (recusado ou sem resposta a tempo)."
                    .to_string()
            }
            Self::Status(401 | 403) => {
                format!("O endpoint recusou a credencial (HTTP {}).", self.code())
            }
            Self::Status(404) => {
                "O endpoint não tem /models (HTTP 404). Confira a URL base.".to_string()
            }
            Self::Status(_) => format!("O endpoint respondeu HTTP {}.", self.code()),
            Self::TooLarge => "A resposta do endpoint passou do limite de 1 MiB.".to_string(),
            Self::BadJson | Self::BadShape => {
                "A resposta não é uma lista de modelos reconhecível.".to_string()
            }
            Self::Cancelled => "A consulta foi cancelada.".to_string(),
            Self::InvalidUrl => {
                "URL inválida: use http:// ou https://, sem usuário e senha na URL.".to_string()
            }
        }
    }

    fn code(&self) -> u16 {
        match self {
            Self::Status(code) => *code,
            _ => 0,
        }
    }
}

fn request_error(error: ureq::Error) -> FetchError {
    match error {
        ureq::Error::BadUri(_) => FetchError::InvalidUrl,
        ureq::Error::StatusCode(status) => FetchError::Status(status),
        ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge,
        ureq::Error::Timeout(_)
        | ureq::Error::ConnectionFailed
        | ureq::Error::HostNotFound
        | ureq::Error::Io(_) => FetchError::Unreachable,
        _ => FetchError::Unreachable,
    }
}

fn body_error(error: ureq::Error) -> FetchError {
    match error {
        ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge,
        ureq::Error::Timeout(_)
        | ureq::Error::ConnectionFailed
        | ureq::Error::HostNotFound
        | ureq::Error::Io(_) => FetchError::Unreachable,
        _ => FetchError::BadShape,
    }
}

fn get_json(
    url: &Url,
    key: Option<&str>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> std::result::Result<Value, FetchError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(FetchError::Cancelled);
    }
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(timeout))
        .timeout_connect(Some(timeout.min(CONNECT_TIMEOUT)))
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let mut request = agent
        .get(url.as_str())
        .header("Accept", "application/json")
        .header("User-Agent", concat!("dish/", env!("CARGO_PKG_VERSION")));
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let mut response = request.call().map_err(request_error)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(FetchError::Cancelled);
    }
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(FetchError::Status(status));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(RESPONSE_LIMIT)
        .read_to_vec()
        .map_err(body_error)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(FetchError::Cancelled);
    }
    serde_json::from_slice(&bytes).map_err(|_| FetchError::BadJson)
}

/// Valida e normaliza a URL base: HTTP(S), host presente, sem credenciais,
/// consulta ou fragmento. Barras finais do caminho são removidas.
pub fn parse_base_url(input: &str) -> std::result::Result<Url, FetchError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(FetchError::InvalidUrl);
    }
    let mut url = Url::parse(input).map_err(|_| FetchError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::InvalidUrl);
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(FetchError::InvalidUrl);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(FetchError::InvalidUrl);
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(if path.is_empty() { "/" } else { path.as_str() });
    Ok(url)
}

fn append_segments(base: &Url, segments: &[&str]) -> std::result::Result<Url, FetchError> {
    let mut url = base.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| FetchError::InvalidUrl)?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

fn model_ids(
    value: &Value,
    container: &str,
    fields: &[&str],
) -> std::result::Result<Vec<String>, FetchError> {
    let entries = value
        .get(container)
        .and_then(Value::as_array)
        .ok_or(FetchError::BadShape)?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for entry in entries {
        let Some(object) = entry.as_object() else {
            continue;
        };
        let Some(id) = fields.iter().find_map(|field| {
            object
                .get(*field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
        }) else {
            continue;
        };
        if seen.insert(id.to_string()) {
            ids.push(id.to_string());
            if ids.len() >= MAX_MODELS {
                break;
            }
        }
    }
    Ok(ids)
}

/// Procura um serviço local. Conexão recusada, tempo esgotado e cancelamento
/// significam apenas "não encontrado"; forma inesperada vira `Failed`.
pub fn detect(
    service: LocalService,
    root: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Detection {
    match detect_inner(service, root, timeout, cancel) {
        Ok(models) => Detection::Found { service, models },
        Err(FetchError::Unreachable | FetchError::Cancelled) => Detection::NotFound { service },
        Err(error) => Detection::Failed {
            service,
            message: error.message(),
        },
    }
}

fn detect_inner(
    service: LocalService,
    root: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> std::result::Result<Vec<String>, FetchError> {
    let base = parse_base_url(root)?;
    match service {
        LocalService::Ollama => {
            let url = append_segments(&base, &["api", "tags"])?;
            let value = get_json(&url, None, timeout, cancel)?;
            model_ids(&value, "models", &["name", "model"])
        }
        LocalService::LmStudio => {
            let url = append_segments(&base, &["v1", "models"])?;
            let value = get_json(&url, None, timeout, cancel)?;
            model_ids(&value, "data", &["id"])
        }
    }
}

/// Lista os modelos de um endpoint já configurado, no formato OpenAI.
pub fn list_models(
    endpoint_base: &str,
    _api: Api,
    key: Option<&str>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> std::result::Result<Vec<String>, FetchError> {
    let base = parse_base_url(endpoint_base)?;
    let url = append_segments(&base, &["models"])?;
    let value = get_json(&url, key, timeout, cancel)?;
    model_ids(&value, "data", &["id"])
}

/// Valida o identificador, a URL e a credencial contra o endpoint. Devolve os
/// modelos que ele listou; qualquer erro já é uma mensagem para a tela.
pub fn validate(
    endpoint: &Endpoint,
    timeout: Duration,
    cancel: &AtomicBool,
) -> std::result::Result<Vec<String>, String> {
    validate_provider_id(&endpoint.provider)?;
    let result = parse_base_url(&endpoint.base_url)
        .map_err(|error| error.message())
        .and_then(|_| {
            list_models(
                &endpoint.base_url,
                endpoint.api,
                endpoint.key.as_deref(),
                timeout,
                cancel,
            )
            .map_err(|error| error.message())
        });
    result.map_err(|message| hide_key(message, endpoint.key.as_deref()))
}

fn validate_provider_id(provider: &str) -> std::result::Result<(), String> {
    if provider.is_empty() {
        return Err("O identificador do provedor é obrigatório.".to_string());
    }
    if provider.len() > 64 {
        return Err("O identificador do provedor pode ter no máximo 64 caracteres.".to_string());
    }
    if !provider.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        return Err(
            "O identificador do provedor só pode ter letras minúsculas, números, ponto, hífen e sublinhado."
                .to_string(),
        );
    }
    Ok(())
}

fn hide_key(message: String, key: Option<&str>) -> String {
    match key.filter(|key| !key.is_empty()) {
        Some(key) if message.contains(key) => message.replace(key, "<oculta>"),
        _ => message,
    }
}

/// Diretório do agente do Pi, respeitando `PI_CODING_AGENT_DIR`.
pub fn agent_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let value = std::env::var("PI_CODING_AGENT_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "~/.pi/agent".to_string());
    Some(crate::sessions::expand_path(&value, &cwd))
}

pub fn models_path() -> Option<PathBuf> {
    agent_dir().map(|dir| dir.join(SAVE_FILE))
}

fn invalid_models() -> anyhow::Error {
    anyhow!("models.json não é um JSON válido; nada foi alterado.")
}

/// Mescla o endpoint no conteúdo existente sem descartar nada que o Pi tenha
/// gravado. Devolve o texto pronto (com quebra de linha final).
///
/// Usado pelos testes; `save` chama `merge_inner` para também saber quantos
/// modelos entraram.
#[cfg(test)]
pub fn merge(existing: Option<&str>, endpoint: &Endpoint) -> Result<String> {
    merge_inner(existing, endpoint).map(|(text, _)| text)
}

fn merge_inner(existing: Option<&str>, endpoint: &Endpoint) -> Result<(String, usize)> {
    let mut root = match existing {
        Some(text) => {
            let value: Value = serde_json::from_str(text).map_err(|_| invalid_models())?;
            if !value.is_object() {
                return Err(invalid_models());
            }
            value
        }
        None => json!({
            "$schema": SCHEMA,
            "providers": Map::new(),
        }),
    };
    let root = root.as_object_mut().ok_or_else(invalid_models)?;
    let providers = root
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(invalid_models)?;
    let provider = providers
        .entry(endpoint.provider.clone())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(invalid_models)?;
    if provider
        .get("models")
        .is_some_and(|models| !models.is_array())
    {
        return Err(invalid_models());
    }

    provider.insert(
        "baseUrl".to_string(),
        Value::String(endpoint.base_url.clone()),
    );
    provider.insert(
        "api".to_string(),
        Value::String(endpoint.api.id().to_string()),
    );
    match endpoint.key.as_deref().filter(|key| !key.is_empty()) {
        Some(key) => {
            provider.insert("apiKey".to_string(), Value::String(key.to_string()));
        }
        None => {
            if !provider.contains_key("apiKey") {
                provider.insert("apiKey".to_string(), Value::String("local".to_string()));
            }
        }
    }
    let models = provider
        .entry("models")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(invalid_models)?;
    let mut seen: HashSet<String> = models
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let mut added = 0;
    for id in &endpoint.models {
        if id.trim().is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        models.push(json!({ "id": id }));
        added += 1;
    }
    let mut text = serde_json::to_string_pretty(&root)
        .context("não foi possível gerar o conteúdo de models.json")?;
    text.push('\n');
    Ok((text, added))
}

/// O que mudou no arquivo. `added_models` conta só entradas novas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOutcome {
    pub created: bool,
    pub added_models: usize,
}

/// Grava `models.json` atomicamente. Se o arquivo já existia, guarda os bytes
/// originais em `models.json.dish-backup` antes de substituí-lo.
pub fn save(path: &Path, endpoint: &Endpoint) -> Result<SaveOutcome> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("caminho de models.json inválido"))?;
    let existing = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("não foi possível ler models.json"),
    };
    let existing_text = match existing.as_ref() {
        Some(bytes) => Some(std::str::from_utf8(bytes).map_err(|_| invalid_models())?),
        None => None,
    };
    let (merged, added_models) = merge_inner(existing_text, endpoint)?;
    fs::create_dir_all(parent).context("não foi possível criar o diretório do agente")?;
    if let Some(bytes) = existing.as_ref() {
        write_atomic(parent, SAVE_BACKUP, bytes)
            .context("não foi possível gravar o backup de models.json")?;
    }
    write_atomic(parent, name, merged.as_bytes()).context("não foi possível gravar models.json")?;
    Ok(SaveOutcome {
        created: existing.is_none(),
        added_models,
    })
}

fn write_atomic(directory: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut last_error = None;
    for _ in 0..SAVE_TEMP_ATTEMPTS {
        let temporary = directory.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let written = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temporary, directory.join(name)) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        return Ok(());
    }
    Err(last_error
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow!("não foi possível criar um arquivo temporário")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    struct Fixture {
        port: u16,
        requests: Arc<Mutex<Vec<String>>>,
        shutdown: Arc<AtomicBool>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(("127.0.0.1", self.port));
        }
    }

    fn fixture<F>(reply: F) -> Fixture
    where
        F: Fn(&str) -> (u16, String) + Send + Sync + 'static,
    {
        start_fixture(reply, false)
    }

    fn silent_fixture() -> Fixture {
        start_fixture(|_| (200, String::new()), true)
    }

    fn start_fixture<F>(reply: F, silent: bool) -> Fixture
    where
        F: Fn(&str) -> (u16, String) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let records = Arc::clone(&requests);
        let stop = Arc::clone(&shutdown);
        let reply = Arc::new(reply);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if silent {
                            hold_open(&mut stream, &stop);
                            continue;
                        }
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let head = read_head(&mut stream);
                        records.lock().unwrap().push(head.clone());
                        let path = request_path(&head);
                        let (status, body) = reply(&path);
                        let response = format!(
                            "HTTP/1.1 {status} fixture\r\n\
                             Content-Type: application/json\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Fixture {
            port,
            requests,
            shutdown,
        }
    }

    fn hold_open(stream: &mut TcpStream, stop: &AtomicBool) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
        let mut byte = [0u8; 1];
        while !stop.load(Ordering::SeqCst) {
            match stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            }
        }
    }

    fn read_head(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        let mut buffer = [0u8; 1024];
        while data.len() < 16 * 1024 {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    data.extend_from_slice(&buffer[..read]);
                    if data.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&data).into_owned()
    }

    fn request_path(head: &str) -> String {
        head.lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/")
            .to_string()
    }

    fn root(port: u16) -> String {
        format!("http://127.0.0.1:{port}")
    }

    fn endpoint(
        provider: &str,
        base_url: String,
        api: Api,
        key: Option<&str>,
        models: &[&str],
    ) -> Endpoint {
        Endpoint {
            provider: provider.to_string(),
            base_url,
            api,
            key: key.map(str::to_string),
            models: models.iter().map(|model| model.to_string()).collect(),
        }
    }

    fn test_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dish-providers-local-{label}-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn assert_no_temp_files(directory: &Path) {
        for entry in fs::read_dir(directory).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().ends_with(".tmp"),
                "arquivo temporário deixado para trás: {name:?}"
            );
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn detects_ollama_models() {
        let server = fixture(|path| {
            if path == "/api/tags" {
                (
                    200,
                    r#"{"models":[{"name":"qwen2.5-coder:7b"},{"name":"llama3.2:3b"}]}"#
                        .to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });
        let detection = detect(
            LocalService::Ollama,
            &root(server.port),
            DETECT_TIMEOUT,
            &AtomicBool::new(false),
        );
        assert_eq!(
            detection,
            Detection::Found {
                service: LocalService::Ollama,
                models: vec!["qwen2.5-coder:7b".to_string(), "llama3.2:3b".to_string()],
            }
        );
        let requests = server.requests.lock().unwrap();
        assert!(
            requests[0].starts_with("GET /api/tags "),
            "{:?}",
            requests[0]
        );
    }

    #[test]
    fn detects_lm_studio_models() {
        let server = fixture(|path| {
            if path == "/v1/models" {
                (
                    200,
                    r#"{"data":[{"id":"local-model"},{"id":"outro-modelo"}]}"#.to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });
        let detection = detect(
            LocalService::LmStudio,
            &root(server.port),
            DETECT_TIMEOUT,
            &AtomicBool::new(false),
        );
        assert_eq!(
            detection,
            Detection::Found {
                service: LocalService::LmStudio,
                models: vec!["local-model".to_string(), "outro-modelo".to_string()],
            }
        );
    }

    #[test]
    fn refused_port_is_not_found_not_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let detection = detect(
            LocalService::LmStudio,
            &root(port),
            Duration::from_millis(500),
            &AtomicBool::new(false),
        );
        assert_eq!(
            detection,
            Detection::NotFound {
                service: LocalService::LmStudio,
            }
        );
    }

    #[test]
    fn silent_server_times_out_as_not_found() {
        let server = silent_fixture();
        let started = Instant::now();
        let detection = detect(
            LocalService::Ollama,
            &root(server.port),
            Duration::from_millis(300),
            &AtomicBool::new(false),
        );
        assert_eq!(
            detection,
            Detection::NotFound {
                service: LocalService::Ollama,
            }
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn cancelled_detection_returns_immediately() {
        let cancel = AtomicBool::new(true);
        let started = Instant::now();
        let detection = detect(
            LocalService::Ollama,
            "http://127.0.0.1:9",
            DETECT_TIMEOUT,
            &cancel,
        );
        assert_eq!(
            detection,
            Detection::NotFound {
                service: LocalService::Ollama,
            }
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn list_models_maps_statuses_and_body_problems() {
        let cancel = AtomicBool::new(false);
        let unauthorized = fixture(|_| (401, r#"{"error":"unauthorized"}"#.to_string()));
        let error = list_models(
            &format!("{}/v1", root(unauthorized.port)),
            Api::OpenAiCompletions,
            Some("sk-test"),
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error, FetchError::Status(401));

        let broken = fixture(|_| (500, r#"{"error":"boom"}"#.to_string()));
        let error = list_models(
            &format!("{}/v1", root(broken.port)),
            Api::OpenAiCompletions,
            None,
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error, FetchError::Status(500));

        let huge = fixture(|_| {
            (
                200,
                format!(
                    r#"{{"data":[{{"id":"{}"}}]}}"#,
                    "x".repeat(1024 * 1024 + 128)
                ),
            )
        });
        let error = list_models(
            &format!("{}/v1", root(huge.port)),
            Api::OpenAiCompletions,
            None,
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error, FetchError::TooLarge);

        let garbage = fixture(|_| (200, "not json".to_string()));
        let error = list_models(
            &format!("{}/v1", root(garbage.port)),
            Api::OpenAiCompletions,
            None,
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        assert!(matches!(error, FetchError::BadJson | FetchError::BadShape));

        let wrong_shape = fixture(|_| (200, r#"{"data":{}}"#.to_string()));
        let error = list_models(
            &format!("{}/v1", root(wrong_shape.port)),
            Api::OpenAiCompletions,
            None,
            Duration::from_secs(2),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error, FetchError::BadShape);
    }

    #[test]
    fn credential_is_sent_only_as_header_and_never_leaks() {
        let key = "sk-test-SECRET-123456";
        let server = fixture(move |_| (401, format!(r#"{{"error":"chave recusada: {key}"}}"#)));
        let endpoint = endpoint(
            "laboratorio",
            format!("{}/v1", root(server.port)),
            Api::OpenAiCompletions,
            Some(key),
            &[],
        );
        let message =
            validate(&endpoint, Duration::from_secs(2), &AtomicBool::new(false)).unwrap_err();
        assert!(!message.contains("SECRET"), "{message}");
        assert!(!message.contains(key), "{message}");
        let debug = format!("{endpoint:?}");
        assert!(!debug.contains(key), "{debug}");
        assert!(debug.contains("<oculta>"), "{debug}");
        let requests = server.requests.lock().unwrap();
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test-secret-123456"),
            "{:?}",
            requests[0]
        );
    }

    #[test]
    fn validation_success_and_input_errors() {
        let server = fixture(|path| {
            if path == "/v1/models" {
                (
                    200,
                    r#"{"data":[{"id":"modelo-a"},{"id":"modelo-b"}]}"#.to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });
        let valid = endpoint(
            "laboratorio",
            format!("{}/v1/", root(server.port)),
            Api::OpenAiCompletions,
            Some("sk-test"),
            &[],
        );
        assert_eq!(
            validate(&valid, Duration::from_secs(2), &AtomicBool::new(false)).unwrap(),
            vec!["modelo-a".to_string(), "modelo-b".to_string()]
        );

        let long = "a".repeat(65);
        for provider in ["", "Mau Provedor", long.as_str(), "acentuação"] {
            let endpoint = endpoint(
                provider,
                root(server.port),
                Api::OpenAiCompletions,
                None,
                &[],
            );
            let message = validate(
                &endpoint,
                Duration::from_millis(100),
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert!(message.contains("provedor"), "{message}");
        }

        for base in [
            "",
            "ftp://127.0.0.1/models",
            "http://user:pass@127.0.0.1/v1",
            "http://127.0.0.1/v1?chave=1",
            "http://127.0.0.1/v1#fragmento",
        ] {
            let endpoint = endpoint(
                "laboratorio",
                base.to_string(),
                Api::OpenAiCompletions,
                None,
                &[],
            );
            let message = validate(
                &endpoint,
                Duration::from_millis(100),
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert!(message.contains("URL"), "{message}");
        }
    }

    #[test]
    fn merge_creates_a_new_file() {
        let ollama = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            Some("ollama"),
            &["qwen2.5-coder:7b"],
        );
        let text = merge(None, &ollama).unwrap();
        assert!(text.ends_with('\n'));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["$schema"], SCHEMA);
        let provider = &value["providers"]["ollama"];
        assert_eq!(provider["baseUrl"], "http://127.0.0.1:11434/v1");
        assert_eq!(provider["api"], "openai-completions");
        assert_eq!(provider["apiKey"], "ollama");
        assert_eq!(provider["models"], json!([{"id": "qwen2.5-coder:7b"}]));

        let generic = endpoint(
            "proxy",
            "https://exemplo.invalid/v1".to_string(),
            Api::OpenAiResponses,
            None,
            &[],
        );
        let text = merge(None, &generic).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["providers"]["proxy"]["api"], "openai-responses");
        assert_eq!(value["providers"]["proxy"]["apiKey"], "local");
    }

    #[test]
    fn merge_preserves_unknown_fields_and_order() {
        let original = r#"{
  "$schema": "https://exemplo.invalid/schema.json",
  "customTop": {"keep": true},
  "providers": {
    "other": {
      "baseUrl": "https://exemplo.invalid/v1",
      "api": "openai-completions",
      "apiKey": "other-key",
      "models": [{"id": "keep-me", "name": "Keep"}]
    },
    "ollama": {
      "name": "Meu Ollama",
      "baseUrl": "http://old.invalid/v1",
      "api": "openai-responses",
      "apiKey": "legacy-key",
      "headers": {"X-Dish": "1"},
      "models": [
        {"id": "existing", "name": "Existing", "reasoning": true, "cost": {"input": 1}},
        {"id": "qwen2.5-coder:7b", "name": "Qwen local"}
      ],
      "modelOverrides": {"qwen2.5-coder:7b": {"contextWindow": 32768}}
    }
  }
}"#;
        let before: Value = serde_json::from_str(original).unwrap();
        let endpoint = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            None,
            &[
                "existing",
                "new-model",
                "new-model",
                "qwen2.5-coder:7b",
                "brand-new",
            ],
        );
        let text = merge(Some(original), &endpoint).unwrap();
        let after: Value = serde_json::from_str(&text).unwrap();
        let top_keys: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(top_keys, ["$schema", "customTop", "providers"]);
        let provider_keys: Vec<&String> = after["providers"].as_object().unwrap().keys().collect();
        assert_eq!(provider_keys, ["other", "ollama"]);
        assert_eq!(after["customTop"], before["customTop"]);
        assert_eq!(after["providers"]["other"], before["providers"]["other"]);

        let provider = &after["providers"]["ollama"];
        let old_provider = &before["providers"]["ollama"];
        assert_eq!(provider["baseUrl"], "http://127.0.0.1:11434/v1");
        assert_eq!(provider["api"], "openai-completions");
        assert_eq!(provider["apiKey"], "legacy-key");
        assert_eq!(provider["name"], "Meu Ollama");
        assert_eq!(provider["headers"], old_provider["headers"]);
        assert_eq!(provider["modelOverrides"], old_provider["modelOverrides"]);
        assert_eq!(provider["models"][0], old_provider["models"][0]);
        assert_eq!(provider["models"][1], old_provider["models"][1]);
        assert_eq!(
            provider["models"],
            json!([
                {"id": "existing", "name": "Existing", "reasoning": true, "cost": {"input": 1}},
                {"id": "qwen2.5-coder:7b", "name": "Qwen local"},
                {"id": "new-model"},
                {"id": "brand-new"}
            ])
        );
    }

    #[test]
    fn merge_replaces_the_key_when_one_is_given() {
        let original = r#"{"providers":{"ollama":{"apiKey":"old-key","models":[]}}}"#;
        let endpoint = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            Some("new-key"),
            &[],
        );
        let value: Value =
            serde_json::from_str(&merge(Some(original), &endpoint).unwrap()).unwrap();
        assert_eq!(value["providers"]["ollama"]["apiKey"], "new-key");
    }

    #[test]
    fn merge_rejects_corrupt_or_wrong_shapes() {
        let endpoint = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            None,
            &[],
        );
        for existing in [
            "{",
            "[]",
            r#"{"providers": []}"#,
            r#"{"providers": {"ollama": []}}"#,
            r#"{"providers": {"ollama": {"models": {}}}}"#,
        ] {
            let error = merge(Some(existing), &endpoint).unwrap_err();
            assert!(error.to_string().contains("models.json"), "{error}");
            assert!(!error.to_string().contains(existing), "{error}");
        }
    }

    #[test]
    fn save_refuses_a_corrupt_file_without_touching_it() {
        let directory = test_dir("save-corrupt");
        let path = directory.join(SAVE_FILE);
        let original = b"{ not json";
        fs::write(&path, original).unwrap();
        let endpoint = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            Some("ollama"),
            &["qwen2.5-coder:7b"],
        );
        let error = save(&path, &endpoint).unwrap_err();
        assert!(error.to_string().contains("models.json"), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!directory.join(SAVE_BACKUP).exists());
        assert_no_temp_files(&directory);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn save_backs_up_and_replaces_an_existing_file() {
        let directory = test_dir("save-existing");
        let path = directory.join(SAVE_FILE);
        let original = br#"{"providers":{"other":{"models":[{"id":"keep"}]}}}"#;
        fs::write(&path, original).unwrap();
        let endpoint = endpoint(
            "ollama",
            "http://127.0.0.1:11434/v1".to_string(),
            Api::OpenAiCompletions,
            Some("ollama"),
            &["qwen2.5-coder:7b"],
        );
        let outcome = save(&path, &endpoint).unwrap();
        assert!(!outcome.created);
        assert_eq!(outcome.added_models, 1);
        let backup = directory.join(SAVE_BACKUP);
        assert_eq!(fs::read(&backup).unwrap(), original);
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            value["providers"]["ollama"]["models"],
            json!([{"id": "qwen2.5-coder:7b"}])
        );
        assert_eq!(
            value["providers"]["other"]["models"],
            json!([{"id": "keep"}])
        );
        assert_no_temp_files(&directory);
        #[cfg(unix)]
        {
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&backup), 0o600);
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn save_creates_a_missing_directory() {
        let directory = test_dir("save-create");
        let path = directory.join("agent").join(SAVE_FILE);
        let endpoint = endpoint(
            "lmstudio",
            "http://127.0.0.1:1234/v1".to_string(),
            Api::OpenAiCompletions,
            Some("lm-studio"),
            &["modelo-local"],
        );
        let outcome = save(&path, &endpoint).unwrap();
        assert!(outcome.created);
        assert_eq!(outcome.added_models, 1);
        assert!(path.is_file());
        assert!(path.parent().unwrap().is_dir());
        assert_no_temp_files(path.parent().unwrap());
        fs::remove_dir_all(directory).unwrap();
    }
}
