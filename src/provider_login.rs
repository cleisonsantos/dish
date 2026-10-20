//! Login de provedor conduzido pelo Pi: perguntas, avisos e desfecho.
//!
//! Fase 3 do #69. O Dish não conduz OAuth nem guarda credencial: ele abre o
//! adaptador (`scripts/pi-auth-adapter.mjs`) como um processo de vida longa,
//! repassa as perguntas do Pi para a interface e envia as respostas de volta.
//! A resposta de uma pergunta `secret` viaja em uma direção só — nada do que foi
//! digitado é guardado, registrado ou devolvido pelo protocolo. O contrato está
//! em `docs/provider-auth.md`.
//!
//! Uma sessão por login, descartada no fim; `Drop` mata o processo para não
//! deixar filho órfão.

use anyhow::{anyhow, Result};
use async_channel::Receiver;
use serde_json::Value;
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

/// Qual método de autenticação o usuário escolheu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    OAuth,
}

impl AuthKind {
    pub fn wire(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::OAuth => "oauth",
        }
    }
}

/// O que o Pi precisa perguntar para concluir o login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskKind {
    /// Campo livre, como um id de conta.
    Text,
    /// Campo que não deve ser exibido nem guardado.
    Secret,
    /// Escolha entre opções do provedor.
    Select,
    /// Código colado de um redirecionamento OAuth.
    ManualCode,
}

impl AskKind {
    fn parse(value: &str) -> Self {
        match value {
            "secret" => Self::Secret,
            "select" => Self::Select,
            "manual_code" => Self::ManualCode,
            _ => Self::Text,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// Uma pergunta do Pi à espera de resposta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub prompt: u64,
    pub kind: AskKind,
    pub message: String,
    pub placeholder: Option<String>,
    pub options: Vec<AskOption>,
}

/// O que acontece enquanto o login corre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Info {
        message: Option<String>,
        links: Vec<String>,
    },
    /// O endereço de autorização; a interface oferece abrir no navegador.
    AuthUrl { url: String, instructions: Option<String> },
    DeviceCode {
        user_code: String,
        verification_uri: Option<String>,
        interval_seconds: u64,
    },
    Progress { message: Option<String> },
}

/// Desfecho de um login. Não carrega credencial: o Pi já gravou a dele.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginOutcome {
    pub provider: String,
    pub auth_type: String,
}

/// Uma linha do adaptador, já interpretada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Ask(Ask),
    Notice(Notice),
    Done(LoginOutcome),
    Failed(String),
    /// Linha de outra operação, ou algo que esta versão não conhece.
    Ignored,
}

/// Lê uma linha do adaptador copiando apenas os campos do contrato.
///
/// Tolerante de propósito: mensagem desconhecida vira `Ignored` em vez de erro, e
/// nada além do que está nomeado aqui entra na estrutura.
pub fn parse_message(line: &str) -> Message {
    let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
        return Message::Ignored;
    };
    if let Some(prompt) = value.get("prompt").and_then(Value::as_u64) {
        return Message::Ask(Ask {
            prompt,
            kind: value
                .get("ask")
                .and_then(Value::as_str)
                .map(AskKind::parse)
                .unwrap_or(AskKind::Text),
            message: text(&value, "message"),
            placeholder: optional_text(&value, "placeholder"),
            options: value
                .get("options")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|option| {
                    Some(AskOption {
                        id: option.get("id").and_then(Value::as_str)?.to_string(),
                        label: text(option, "label"),
                        description: optional_text(option, "description"),
                    })
                })
                .collect(),
        });
    }
    if let Some(kind) = value.get("notify").and_then(Value::as_str) {
        return Message::Notice(match kind {
            "auth_url" => Notice::AuthUrl {
                url: text(&value, "url"),
                instructions: optional_text(&value, "instructions"),
            },
            "device_code" => Notice::DeviceCode {
                user_code: text(&value, "userCode"),
                verification_uri: optional_text(&value, "verificationUri"),
                interval_seconds: value.get("intervalSeconds").and_then(Value::as_u64).unwrap_or(0),
            },
            "progress" => Notice::Progress {
                message: optional_text(&value, "message"),
            },
            _ => Notice::Info {
                message: optional_text(&value, "message"),
                links: value
                    .get("links")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|link| link.get("url").and_then(Value::as_str).map(str::to_string))
                    .collect(),
            },
        });
    }
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Message::Done(LoginOutcome {
            provider: value
                .get("data")
                .map(|data| text(data, "provider"))
                .unwrap_or_default(),
            auth_type: value
                .get("data")
                .map(|data| text(data, "type"))
                .unwrap_or_default(),
        }),
        Some(false) => Message::Failed(failure_text(&value)),
        None => Message::Ignored,
    }
}

/// Código fechado do adaptador vira uma frase; o `detail` cru não é exibido.
fn failure_text(value: &Value) -> String {
    match value.get("error").and_then(Value::as_str).unwrap_or_default() {
        "cancelled" => "Login cancelado.".into(),
        "busy" => "Já existe um login em andamento.".into(),
        "unsupported_type" => "Este provedor não oferece esse tipo de login.".into(),
        "unknown_provider" => "O Pi não conhece esse provedor.".into(),
        "package_not_found" => "Não encontrei a instalação do Pi.".into(),
        "package_invalid" => "O caminho configurado para o Pi não contém o pacote.".into(),
        "login_failed" => "O provedor recusou a credencial.".into(),
        "logout_failed" => "O Pi não conseguiu remover a credencial.".into(),
        "unknown_prompt" => "A resposta não corresponde a nenhuma pergunta.".into(),
        "bad_request" => "Pedido inválido para o adaptador.".into(),
        "" => "O login falhou sem informar o motivo.".into(),
        code => format!("O login falhou ({code})."),
    }
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn optional_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Um adaptador em execução, com um login em andamento.
pub struct LoginSession {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<Message>,
    next_id: u64,
}

impl LoginSession {
    /// Abre o adaptador e começa o login de um provedor.
    pub fn start(provider: &str, kind: AuthKind) -> Result<Self> {
        let node = crate::providers::find_node()
            .ok_or_else(|| anyhow!("Node não está no PATH, então o Dish não consegue falar com o Pi."))?;
        let adapter = crate::providers::materialize_adapter()?;
        let mut command = Command::new(node);
        command
            .arg(&adapter)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::platform::hide_console(&mut command);
        crate::process::configure_group(&mut command);
        let mut child = command.spawn().map_err(|_| anyhow!("Não foi possível executar o Node."))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Sem entrada para o adaptador."))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Sem saída do adaptador."))?;
        let (tx, events) = async_channel::bounded::<Message>(64);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let line = match crate::limits::read_line_limited(
                    &mut reader,
                    crate::limits::MAX_LOGIN_FRAME_BYTES,
                    false,
                ) {
                    Ok(crate::limits::BoundedLine::Line(line)) => line,
                    Ok(crate::limits::BoundedLine::TooLong { .. }) => {
                        let _ = tx.try_send(Message::Failed(
                            "O adaptador enviou uma resposta grande demais.".into(),
                        ));
                        break;
                    }
                    Ok(crate::limits::BoundedLine::Eof) | Err(_) => break,
                };
                let line = String::from_utf8_lossy(&line);
                if tx.send_blocking(parse_message(&line)).is_err() {
                    break;
                }
            }
        });
        let mut session = Self { child, stdin, events, next_id: 1 };
        session.send(&serde_json::json!({
            "id": session.next_id,
            "op": "login",
            "provider": provider,
            "type": kind.wire(),
        }))?;
        Ok(session)
    }

    /// Canal de perguntas, avisos e desfecho.
    pub fn events(&self) -> Receiver<Message> {
        self.events.clone()
    }

    /// Envia a resposta de uma pergunta. O valor não é guardado em lugar nenhum.
    pub fn reply(&mut self, prompt: u64, value: &str) -> Result<()> {
        self.next_id += 1;
        self.send(&serde_json::json!({
            "id": self.next_id,
            "op": "reply",
            "prompt": prompt,
            "value": value,
        }))
    }

    /// Cancela o login em andamento; o processo é encerrado em seguida.
    pub fn cancel(&mut self) -> Result<()> {
        self.next_id += 1;
        self.send(&serde_json::json!({
            "id": self.next_id,
            "op": "cancel",
            "target": 1,
        }))
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        let mut line = message.to_string();
        line.push('\n');
        if line.len() > crate::limits::MAX_LOGIN_COMMAND_BYTES {
            return Err(anyhow!("A resposta do login é grande demais."));
        }
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|_| anyhow!("O adaptador encerrou a entrada."))
    }
}

impl Drop for LoginSession {
    fn drop(&mut self) {
        crate::process::terminate_tree(&mut self.child);
    }
}

/// Remove a credencial guardada de um provedor, sem manter processo aberto.
pub fn logout(provider: &str) -> Result<()> {
    let node = crate::providers::find_node()
        .ok_or_else(|| anyhow!("Node não está no PATH, então o Dish não consegue falar com o Pi."))?;
    let adapter = crate::providers::materialize_adapter()?;
    let mut command = Command::new(node);
    command.arg(&adapter);
    let body = format!("{}\n", serde_json::json!({ "id": 1, "op": "logout", "provider": provider }));
    let output = crate::providers::run_with_stdin(command, &body, std::time::Duration::from_secs(20))?;
    match parse_message(&output) {
        Message::Done(_) => Ok(()),
        Message::Failed(reason) => Err(anyhow!(reason)),
        _ => Err(anyhow!("O Pi não respondeu ao pedido de remoção.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_question_is_parsed_without_any_value() {
        let line = r#"{"id":2,"prompt":7,"ask":"secret","message":"Cole a chave",
            "placeholder":"sk-ant-…","options":[],"value":"sk-ant-NAO-PODE-VAZAR"}"#;
        let Message::Ask(ask) = parse_message(line) else { panic!("esperava pergunta") };
        assert_eq!(ask.prompt, 7);
        assert_eq!(ask.kind, AskKind::Secret);
        assert_eq!(ask.message, "Cole a chave");
        assert_eq!(ask.placeholder.as_deref(), Some("sk-ant-…"));
        let debug = format!("{ask:?}");
        assert!(!debug.contains("NAO-PODE-VAZAR"), "{debug}");
    }

    #[test]
    fn a_select_question_carries_its_options() {
        let line = r#"{"id":1,"prompt":1,"ask":"select","message":"Região","options":[
            {"id":"br","label":"Brasil","description":"southamerica-east1"},
            {"id":"us","label":"EUA"}]}"#;
        let Message::Ask(ask) = parse_message(line) else { panic!("esperava pergunta") };
        assert_eq!(ask.kind, AskKind::Select);
        assert_eq!(ask.options.len(), 2);
        assert_eq!(ask.options[0].id, "br");
        assert_eq!(ask.options[0].description.as_deref(), Some("southamerica-east1"));
        assert_eq!(ask.options[1].description, None);
    }

    #[test]
    fn oauth_notices_arrive_in_their_own_shape() {
        let url = parse_message(r#"{"id":1,"notify":"auth_url","url":"https://exemplo",
            "instructions":"Autorize"}"#);
        assert_eq!(
            url,
            Message::Notice(Notice::AuthUrl {
                url: "https://exemplo".into(),
                instructions: Some("Autorize".into()),
            })
        );
        let device = parse_message(r#"{"id":1,"notify":"device_code","userCode":"ABCD-1234",
            "verificationUri":"https://exemplo/device","intervalSeconds":5,"expiresInSeconds":900}"#);
        assert_eq!(
            device,
            Message::Notice(Notice::DeviceCode {
                user_code: "ABCD-1234".into(),
                verification_uri: Some("https://exemplo/device".into()),
                interval_seconds: 5,
            })
        );
        let progress = parse_message(r#"{"id":1,"notify":"progress","message":"Aguardando…"}"#);
        assert_eq!(
            progress,
            Message::Notice(Notice::Progress { message: Some("Aguardando…".into()) })
        );
        let info = parse_message(r#"{"id":1,"notify":"info","message":"Veja a ajuda",
            "links":[{"url":"https://ajuda","label":"Ajuda"}]}"#);
        assert_eq!(
            info,
            Message::Notice(Notice::Info {
                message: Some("Veja a ajuda".into()),
                links: vec!["https://ajuda".into()],
            })
        );
    }

    #[test]
    fn a_successful_login_reports_provider_and_type_only() {
        let line = r#"{"id":1,"ok":true,"data":{"provider":"openai","type":"oauth",
            "credential":{"access":"access-token","refresh":"refresh-token"}}}"#;
        let Message::Done(outcome) = parse_message(line) else { panic!("esperava desfecho") };
        assert_eq!(outcome.provider, "openai");
        assert_eq!(outcome.auth_type, "oauth");
        let debug = format!("{outcome:?}");
        assert!(!debug.contains("access-token"), "{debug}");
        assert!(!debug.contains("refresh-token"), "{debug}");
    }

    #[test]
    fn failures_become_portuguese_sentences_without_the_raw_detail() {
        let line = r#"{"id":1,"ok":false,"error":"login_failed",
            "detail":"chave recusada: sk-ant-NAO-PODE-VAZAR"}"#;
        let Message::Failed(message) = parse_message(line) else { panic!("esperava falha") };
        assert_eq!(message, "O provedor recusou a credencial.");
        assert!(!message.contains("NAO-PODE-VAZAR"), "{message}");
        // Código desconhecido não inventa tradução nem vaza texto livre.
        let unknown = parse_message(r#"{"id":1,"ok":false,"error":"algo_novo","detail":"x"}"#);
        assert_eq!(unknown, Message::Failed("O login falhou (algo_novo).".into()));
    }

    #[test]
    fn unknown_and_malformed_lines_are_ignored() {
        assert_eq!(parse_message("{\"id\":1,\"op\":\"status\"}"), Message::Ignored);
        assert_eq!(parse_message("not json"), Message::Ignored);
        assert_eq!(parse_message(""), Message::Ignored);
    }
}
