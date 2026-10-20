//! Quais provedores o Pi conhece, quais têm credencial e de onde ela vem.
//!
//! Fase 2 do #69. O Dish não autentica nada: ele pergunta ao adaptador
//! (`scripts/pi-auth-adapter.mjs`, embutido neste binário) e mostra o estado.
//! Nenhum valor de credencial atravessa esta fronteira — o que entra é
//! *estado* e *origem*, e o que sai é uma lista para a tela de configurações.
//! O contrato está em `docs/provider-auth.md`.
//!
//! Toda chamada aqui é bloqueante e deve rodar na `background_executor`.

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// O adaptador acompanha o binário: embutido no build e gravado em cache na
/// primeira consulta, para não existirem duas cópias para manter em sincronia.
const ADAPTER: &str = include_str!("../scripts/pi-auth-adapter.mjs");
const ADAPTER_FILE: &str = "pi-auth-adapter.mjs";

/// O adaptador importa o SDK do Pi; 20s cobrem uma máquina fria sem travar a
/// tela, e o processo é morto no estouro.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// De onde vem a credencial de um provedor, conforme o SDK do Pi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Salva por `/login` em `auth.json`.
    Stored,
    /// Fornecida em tempo de execução (`--api-key`).
    Runtime,
    /// Variável de ambiente.
    Environment,
    /// Credencial ambiente do provedor (perfis de nuvem, ADC).
    Fallback,
    /// `key` em `models.json`.
    ModelsJsonKey,
    /// Comando (`!…`) em `models.json`.
    ModelsJsonCommand,
    /// Valor que este Dish ainda não conhece.
    Unknown,
}

impl Source {
    fn parse(value: &str) -> Self {
        match value {
            "stored" => Self::Stored,
            "runtime" => Self::Runtime,
            "environment" => Self::Environment,
            "fallback" => Self::Fallback,
            "models_json_key" => Self::ModelsJsonKey,
            "models_json_command" => Self::ModelsJsonCommand,
            _ => Self::Unknown,
        }
    }

    /// Rótulo em português para a lista.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stored => "credencial salva",
            Self::Runtime => "fornecida agora",
            Self::Environment => "variável de ambiente",
            Self::Fallback => "credencial do ambiente",
            Self::ModelsJsonKey => "chave em models.json",
            Self::ModelsJsonCommand => "comando em models.json",
            Self::Unknown => "origem desconhecida",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthMethod {
    /// Nome que o provedor dá ao método, exibido na escolha.
    pub name: String,
    /// Rótulo preferido do provedor para o login, quando existe.
    pub label: Option<String>,
    /// O acesso é por assinatura, não por chave.
    pub subscription: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub id: String,
    pub models: usize,
    pub configured: bool,
    pub source: Option<Source>,
    /// Login interativo por chave de API, quando o provedor oferece.
    pub api_key: Option<AuthMethod>,
    /// Login interativo por conta do provedor (OAuth), quando oferece.
    pub oauth: Option<AuthMethod>,
}

impl Provider {
    /// Métodos que a interface pode oferecer para conectar este provedor.
    pub fn methods(&self) -> Vec<(crate::provider_login::AuthKind, String)> {
        let mut methods = Vec::new();
        if let Some(method) = &self.api_key {
            methods.push((crate::provider_login::AuthKind::ApiKey, method.name.clone()));
        }
        if let Some(method) = &self.oauth {
            let label = method.label.clone().unwrap_or_else(|| method.name.clone());
            methods.push((crate::provider_login::AuthKind::OAuth, label));
        }
        methods
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub pi_version: Option<String>,
    pub providers: Vec<Provider>,
}

impl Snapshot {
    pub fn ready(&self) -> usize {
        self.providers.iter().filter(|provider| provider.configured).count()
    }

    /// Nenhum provedor utilizável: é o primeiro uso que a issue #69 descreve.
    pub fn empty(&self) -> bool {
        self.providers.is_empty() || self.ready() == 0
    }
}

/// O que a tela tem para mostrar depois de consultar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probed {
    Ready(Snapshot),
    /// Sem resposta possível — sem Node, sem pacote do Pi ou consulta falhou.
    /// A tela diz o motivo em português em vez de fingir uma lista vazia.
    Unavailable(String),
}

/// Consulta o adaptador. Bloqueante: chame da `background_executor`.
pub fn probe() -> Probed {
    let Some(node) = find_node() else {
        return Probed::Unavailable(
            "Node não está no PATH, então o Dish não consegue ler o catálogo do Pi.".into(),
        );
    };
    let adapter = match materialize_adapter() {
        Ok(path) => path,
        Err(error) => return Probed::Unavailable(format!("{error:#}")),
    };
    let mut command = Command::new(node);
    command.arg(&adapter).arg("--status");
    let output = match run_bounded(command, PROBE_TIMEOUT) {
        Ok(output) => output,
        Err(error) => return Probed::Unavailable(format!("{error:#}")),
    };
    match parse(&output) {
        Ok(snapshot) => Probed::Ready(snapshot),
        Err(error) => Probed::Unavailable(format!("{error:#}")),
    }
}

/// Lê a resposta do adaptador copiando **apenas** os campos do contrato.
///
/// Tolerante de propósito: campos novos do Pi são ignorados, e nada que não
/// esteja nomeado aqui entra na estrutura — nem para exibir, nem para registrar.
pub fn parse(stdout: &str) -> Result<Snapshot> {
    let root: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| anyhow!("O adaptador não respondeu em JSON."))?;
    if root.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(anyhow!("{}", adapter_error(&root)));
    }
    let data = root.get("data").ok_or_else(|| anyhow!("Resposta sem dados."))?;
    let pi_version = data
        .get("pi")
        .and_then(|pi| pi.get("version"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let providers = data
        .get("providers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            if id.is_empty() {
                return None;
            }
            let configured = entry.get("configured").and_then(Value::as_bool).unwrap_or(false);
            let auth = entry.get("auth");
            Some(Provider {
                id: id.to_string(),
                models: entry.get("models").and_then(Value::as_u64).unwrap_or(0) as usize,
                configured,
                source: entry
                    .get("source")
                    .and_then(Value::as_str)
                    .map(Source::parse)
                    // Sem credencial não há origem a mostrar.
                    .filter(|_| configured),
                api_key: auth.and_then(|auth| method(auth, "api_key")),
                oauth: auth.and_then(|auth| method(auth, "oauth")),
            })
        })
        .collect();
    Ok(Snapshot { pi_version, providers })
}

/// Um método de login só entra se o Pi oferecer um login interativo para ele.
fn method(auth: &Value, key: &str) -> Option<AuthMethod> {
    let entry = auth.get(key)?;
    if entry.is_null() {
        return None;
    }
    let name = entry.get("name").and_then(Value::as_str)?.to_string();
    Some(AuthMethod {
        name,
        label: entry.get("label").and_then(Value::as_str).map(str::to_string),
        subscription: entry.get("subscription").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Código fechado do adaptador vira uma frase; o `detail` cru não é exibido,
/// porque pode carregar caminho da máquina sem ajudar quem está lendo a tela.
fn adapter_error(root: &Value) -> String {
    match root.get("error").and_then(Value::as_str).unwrap_or_default() {
        "package_not_found" => "Não encontrei a instalação do Pi para ler o catálogo.".into(),
        "package_invalid" => "O caminho configurado para o Pi não contém o pacote.".into(),
        "probe_failed" => "O Pi não respondeu ao pedido de estado dos provedores.".into(),
        "bad_json" => "O adaptador recebeu uma entrada inválida.".into(),
        "" => "O adaptador falhou sem informar o motivo.".into(),
        code => format!("O adaptador falhou ({code})."),
    }
}

/// Grava o adaptador embutido em cache e devolve o caminho.
///
/// O conteúdo é o do binário: comparar antes de escrever evita reescrever a cada
/// consulta e mantém a cópia em sincronia quando o Dish é atualizado.
pub(crate) fn materialize_adapter() -> Result<PathBuf> {
    let root = cache_dir().ok_or_else(|| anyhow!("Não encontrei um diretório de cache."))?;
    let directory = root.join("dish");
    crate::fs_security::ensure_private_dir(&directory)
        .map_err(|_| anyhow!("Não foi possível criar o diretório de cache."))?;
    let path = directory.join(ADAPTER_FILE);
    let current = std::fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.len() <= 1024 * 1024)
        .and_then(|_| std::fs::read(&path).ok());
    if current.as_deref() != Some(ADAPTER.as_bytes()) {
        crate::fs_security::write_atomic(&path, ADAPTER.as_bytes())
            .map_err(|_| anyhow!("Não foi possível gravar o adaptador no cache."))?;
    }
    Ok(path)
}

fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let local = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty());
            local.map(PathBuf::from).or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache"))
            })
        })
}

/// Node no `PATH`, com o sufixo do Windows.
pub fn find_node() -> Option<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

/// Executa com prazo, sem bloquear o processo se ele travar.
///
/// Diferente de `installation::run`, uma saída diferente de zero não é erro: o
/// adaptador responde o motivo em JSON no stdout e usa o código para o teste.
pub fn run_bounded(command: Command, timeout: Duration) -> Result<String> {
    let output = crate::process::run(command, timeout, 65_536, None, None)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Como `run_bounded`, mas escreve uma entrada e fecha o stdin do filho.
///
/// Operações sem interação (remover credencial) terminam quando a entrada acaba,
/// então escrever e fechar é o bastante para o adaptador encerrar sozinho.
pub fn run_with_stdin(command: Command, body: &str, timeout: Duration) -> Result<String> {
    let output = crate::process::run(command, timeout, 65_536, None, Some(body.as_bytes()))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_only_the_contract_fields() {
        // O adaptador não deve emitir segredo, e mesmo que emitisse, nada além
        // dos campos nomeados entra na estrutura — nem em Debug.
        let payload = r#"{"ok":true,"data":{
            "pi":{"version":"1.2.0","package":"/home/alguem/.pi"},
            "providers":[
                {"id":"openai","models":44,"configured":true,"source":"stored",
                 "label":"OpenAI","key":"sk-live-NAO-PODE-VAZAR","env":{"OPENAI_API_KEY":"sk-live-NAO-PODE-VAZAR"},
                 "auth":{"api_key":{"name":"OpenAI API key"},
                         "oauth":{"name":"OpenAI","label":"Entrar com a conta","subscription":true,
                                  "token":"eyJhbGciNAO-PODE-VAZAR"}}},
                {"id":"groq","models":2,"configured":false,"source":null,
                 "token":"eyJhbGciNAO-PODE-VAZAR"}],
            "credentials":[{"providerId":"openai","type":"oauth","secret":"sk-live-NAO-PODE-VAZAR"}]
        }}"#;
        let snapshot = parse(payload).expect("resposta válida");
        assert_eq!(snapshot.pi_version.as_deref(), Some("1.2.0"));
        assert_eq!(snapshot.providers.len(), 2);
        assert_eq!(snapshot.ready(), 1);
        let debug = format!("{snapshot:?}");
        assert!(!debug.contains("NAO-PODE-VAZAR"), "{debug}");
        assert!(!debug.contains("sk-live"), "{debug}");
        assert!(!debug.contains("eyJ"), "{debug}");
        let openai = &snapshot.providers[0];
        assert_eq!(openai.id, "openai");
        assert_eq!(openai.models, 44);
        assert_eq!(openai.source, Some(Source::Stored));
        assert_eq!(openai.source.map(Source::label), Some("credencial salva"));
        // O que a interface pode oferecer para conectar.
        assert_eq!(
            openai.methods(),
            vec![
                (crate::provider_login::AuthKind::ApiKey, "OpenAI API key".to_string()),
                (crate::provider_login::AuthKind::OAuth, "Entrar com a conta".to_string()),
            ]
        );
        assert_eq!(openai.oauth.as_ref().map(|method| method.subscription), Some(true));
        // Sem credencial não se anuncia origem; sem método não se oferece botão.
        assert_eq!(snapshot.providers[1].source, None);
        assert!(snapshot.providers[1].methods().is_empty());
    }

    #[test]
    fn parse_maps_errors_to_portuguese_without_the_raw_detail() {
        let payload = r#"{"ok":false,"error":"package_invalid",
            "detail":"/home/alguem/.pi/afetado não contém o pacote"}"#;
        let error = parse(payload).expect_err("erro do adaptador");
        let message = format!("{error:#}");
        assert_eq!(message, "O caminho configurado para o Pi não contém o pacote.");
        assert!(!message.contains("/home/alguem"), "{message}");
    }

    #[test]
    fn parse_accepts_unknown_sources_and_missing_fields() {
        let payload = r#"{"ok":true,"data":{"providers":[
            {"id":"futuro","models":3,"configured":true,"source":"algo_novo"},
            {"id":"parcial"},
            {"models":1,"configured":true},
            {"id":"","models":9}]}}"#;
        let snapshot = parse(payload).expect("resposta válida");
        // Entradas sem id são descartadas; campos ausentes viram padrão.
        assert_eq!(snapshot.providers.len(), 2);
        assert_eq!(snapshot.providers[0].source, Some(Source::Unknown));
        assert_eq!(snapshot.providers[0].source.map(Source::label), Some("origem desconhecida"));
        assert_eq!(snapshot.providers[1].id, "parcial");
        assert!(!snapshot.providers[1].configured);
        assert_eq!(snapshot.pi_version, None);
        assert!(!snapshot.empty(), "há um provedor pronto");
    }

    #[test]
    fn an_empty_catalog_is_the_first_use_case() {
        let snapshot = parse(r#"{"ok":true,"data":{"providers":[]}}"#).expect("válido");
        assert!(snapshot.empty());
        let none_ready = parse(
            r#"{"ok":true,"data":{"providers":[{"id":"groq","models":2,"configured":false}]}}"#,
        )
        .expect("válido");
        assert!(none_ready.empty());
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse("<html>").is_err());
        assert!(parse(r#"{"ok":true}"#).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn a_hanging_probe_is_terminated() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("dish-provider-{}.sh", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 10\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut command = Command::new(&path);
        command.arg("--status");
        let result = run_bounded(command, Duration::from_millis(50));
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "o processo deveria ter sido morto");
    }

    #[test]
    #[cfg(unix)]
    fn a_non_zero_exit_still_returns_the_body() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("dish-provider-err-{}.sh", std::process::id()));
        std::fs::write(&path, "#!/bin/sh\nprintf '{\"ok\":false,\"error\":\"package_not_found\"}'\nexit 4\n")
            .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = run_bounded(Command::new(&path), Duration::from_secs(5)).expect("corpo lido");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            format!("{:#}", parse(&output).expect_err("erro")),
            "Não encontrei a instalação do Pi para ler o catálogo."
        );
    }

    #[test]
    fn the_embedded_adapter_is_the_script() {
        // Se o script mudar de contrato, o embutido muda junto — este teste
        // existe para o `include_str!` não apontar para outro arquivo.
        assert!(ADAPTER.contains("pi-auth-adapter"));
        assert!(ADAPTER.contains("\"status\""));
        assert!(!ADAPTER.is_empty());
    }
}
