//! Servidor MCP stdio independente da janela, com leituras de disco.
use serde_json::{json, Value};
use std::io::{self, Write};

// Confirmado em pi-mcp/dist/protocol/types.js, incluído no Pi 1.5.0.
const PROTOCOL: &str = "2024-11-05";
const PREFERENCE_FIELDS: &[&str] = &[
    "details_open",
    "navigation_open",
    "show_thinking",
    "generated_titles",
    "completion_sound",
    "hidden_models",
    "pinned_sessions",
    "session_order",
    "session_layout",
    "project_order",
    "appearance",
    "reduce_motion",
    "theme",
    "pi_auto_check",
    "pi_auto_install",
    "update_policy",
];
const TOOLS: &[(&str, &str)] = &[
    (
        "dish_status",
        "Estado do Dish a partir do disco, sem janela.",
    ),
    ("dish_preferences", "Preferências permitidas do Dish."),
    (
        "dish_sessions",
        "Sessões salvas, sem conteúdo das mensagens.",
    ),
    (
        "dish_providers",
        "Provedores disponíveis e seu estado de configuração.",
    ),
    (
        "dish_settings_sections",
        "Seções das configurações do Dish.",
    ),
];

fn tools() -> Value {
    json!({"tools": TOOLS.iter().map(|(name, description)| {
        let properties = if *name == "dish_sessions" {
            json!({"cwd":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":500,"default":50}})
        } else { json!({}) };
        json!({"name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"additionalProperties":false},
            "annotations":{"readOnlyHint":true}})
    }).collect::<Vec<_>>()})
}

fn preferences(value: &Value) -> Value {
    Value::Object(
        PREFERENCE_FIELDS
            .iter()
            .filter_map(|name| {
                value
                    .get(*name)
                    .map(|value| ((*name).to_owned(), value.clone()))
            })
            .collect(),
    )
}

fn tool_error(message: &str) -> Value {
    json!({"isError":true,"content":[{"type":"text","text":message}]})
}

fn call(params: &Value) -> Value {
    let Some(params) = params.as_object() else {
        return tool_error("Parâmetros inválidos.");
    };
    if params
        .keys()
        .any(|key| !["name", "arguments", "_meta"].contains(&key.as_str()))
    {
        return tool_error("Parâmetros inválidos.");
    }
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_error("Nome de ferramenta inválido.");
    };
    if !TOOLS.iter().any(|(tool, _)| *tool == name) {
        return tool_error("Ferramenta desconhecida.");
    }
    let empty = json!({});
    let args = params.get("arguments").unwrap_or(&empty);
    let Some(arguments) = args.as_object() else {
        return tool_error("Argumentos inválidos.");
    };
    if arguments
        .keys()
        .any(|key| name != "dish_sessions" || !["cwd", "limit"].contains(&key.as_str()))
    {
        return tool_error("Argumento desconhecido.");
    }
    if arguments
        .get("cwd")
        .is_some_and(|value| value.as_str().is_none())
        || arguments
            .get("limit")
            .is_some_and(|value| value.as_u64().is_none_or(|n| !(1..=500).contains(&n)))
    {
        return tool_error("Filtro ou limite inválido.");
    }
    let prefs = crate::preferences::load();
    let data = match name {
        "dish_preferences" => match serde_json::to_value(&prefs) {
            Ok(value) => preferences(&value),
            Err(_) => return tool_error("Não foi possível ler as preferências."),
        },
        "dish_status" => {
            let pi_version = match crate::providers::probe() {
                crate::providers::Probed::Ready(snapshot) => snapshot.pi_version,
                crate::providers::Probed::Unavailable(_) => None,
            };
            json!({"version":crate::version::release_version(),"pi_version":pi_version,
                "control_protocol":null,"gui":false,"cwd":null,"session":null,
                "settings_section":null,"appearance":prefs.appearance,"theme":prefs.theme,"model":null})
        }
        "dish_sessions" => {
            let Ok(cwd) = std::env::current_dir() else {
                return tool_error("Não foi possível localizar a pasta atual.");
            };
            let filter = args.get("cwd").and_then(Value::as_str);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
            let sessions =
                crate::sessions::discover_cached(&crate::sessions::roots(&cwd, &[]), &[]);
            json!({"sessions":sessions.into_iter().filter(|session|
                filter.is_none_or(|filter| session.cwd == std::path::Path::new(filter))
            ).take(limit).map(|session| json!({
                "path":session.path,"cwd":session.cwd,"title":session.title,"named":session.named,
                "modified":session.modified.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs()),
                "open":false,"running":false,"pinned":prefs.pinned_sessions.contains(&session.path)
            })).collect::<Vec<_>>()})
        }
        "dish_providers" => match crate::providers::probe() {
            crate::providers::Probed::Ready(snapshot) => {
                json!({"providers": snapshot.providers.into_iter().map(|provider| json!({
                "id":provider.id,"models":provider.models,"configured":provider.configured,
                "source":provider.source.map(|source| match source {
                    crate::providers::Source::Stored => "stored",
                    crate::providers::Source::Runtime => "runtime",
                    crate::providers::Source::Environment => "environment",
                    crate::providers::Source::Fallback => "fallback",
                    crate::providers::Source::ModelsJsonKey | crate::providers::Source::ModelsJsonCommand => "models_json",
                    crate::providers::Source::Unknown => "unknown",
                })
            })).collect::<Vec<_>>()})
            }
            // Não encaminhar diagnóstico de subprocesso nem valores desconhecidos.
            crate::providers::Probed::Unavailable(_) => {
                return tool_error("Não foi possível consultar os provedores do Pi.")
            }
        },
        "dish_settings_sections" => json!({"sections": sections()}),
        _ => return tool_error("Ferramenta desconhecida."),
    };
    json!({"isError":false,"content":[{"type":"text","text":json!({"source":"disk","gui":false,"data":data}).to_string()}]})
}

fn sections() -> Vec<Value> {
    use crate::settings::Section;
    const SECTIONS: &[Section] = &[
        Section::App,
        Section::Appearance,
        Section::Navigation,
        Section::Providers,
        Section::Models,
        Section::Sessions,
        Section::Keys,
        Section::About,
    ];
    SECTIONS
        .iter()
        .map(|section| {
            let (id, description) = match section {
                Section::App => ("app", "Comportamento e atualizações do aplicativo."),
                Section::Appearance => ("appearance", "Aparência, tema e movimento."),
                Section::Navigation => {
                    ("navigation", "Ordem e organização dos projetos e sessões.")
                }
                Section::Providers => ("providers", "Provedores disponíveis e configuração."),
                Section::Models => ("models", "Visibilidade dos modelos."),
                Section::Sessions => ("sessions", "Sessões salvas por pasta."),
                Section::Keys => ("keys", "Atalhos de teclado."),
                Section::About => ("about", "Versões do Dish e do Pi."),
            };
            json!({"id":id,"description":description})
        })
        .collect()
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn dispatch(request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    if !request.is_object()
        || request.get("jsonrpc") != Some(&json!("2.0"))
        || request.get("method").and_then(Value::as_str).is_none()
        || id
            .as_ref()
            .is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
        || request
            .get("params")
            .is_some_and(|params| !params.is_object() && !params.is_array())
    {
        return Some(rpc_error(Value::Null, -32600, "Requisição inválida."));
    }
    let id = id?; // Notificações não recebem resposta.
    let empty = json!({});
    let result = match request["method"].as_str().unwrap_or_default() {
        "initialize" => {
            json!({"protocolVersion":PROTOCOL,"capabilities":{"tools":{"listChanged":false}},
            "serverInfo":{"name":"dish","version":crate::version::release_version()}})
        }
        "tools/list" => tools(),
        "tools/call" => call(request.get("params").unwrap_or(&empty)),
        _ => return Some(rpc_error(id, -32601, "Método desconhecido.")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub fn serve() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let response = match crate::limits::read_line_limited(
            &mut input,
            crate::limits::MAX_RPC_FRAME_BYTES,
            true,
        )? {
            crate::limits::BoundedLine::Eof => break,
            crate::limits::BoundedLine::TooLong { .. } => {
                Some(rpc_error(Value::Null, -32600, "Requisição muito grande."))
            }
            crate::limits::BoundedLine::Line(line) => match serde_json::from_slice(&line) {
                Ok(request) => dispatch(request),
                Err(_) => Some(rpc_error(Value::Null, -32700, "JSON inválido.")),
            },
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_excludes_unlisted_and_future_fields() {
        let mut input = serde_json::to_value(crate::preferences::Preferences::default()).unwrap();
        input["future_field"] = json!("never export");
        input["last_update_etag"] = json!("never export");
        let filtered = preferences(&input);
        assert_eq!(filtered.as_object().unwrap().len(), PREFERENCE_FIELDS.len());
        for field in filtered.as_object().unwrap().keys() {
            assert!(PREFERENCE_FIELDS.contains(&field.as_str()));
        }
        assert!(!filtered.to_string().contains("never export"));
    }

    #[test]
    fn protocol_and_tool_contract() {
        let response = dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL);
        assert!(dispatch(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let listing = tools();
        assert_eq!(listing["tools"].as_array().unwrap().len(), 5);
        for tool in listing["tools"].as_array().unwrap() {
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            let schema = tool["inputSchema"].to_string().to_lowercase();
            for term in ["key", "token", "secret", "password", "auth"] {
                assert!(!schema.contains(term));
            }
        }
        for args in [
            json!({"limit":0}),
            json!({"limit":501}),
            json!({"limit":1.5}),
            json!({"cwd":false}),
            json!({"extra":true}),
            json!(null),
        ] {
            assert_eq!(
                call(&json!({"name":"dish_sessions","arguments":args}))["isError"],
                true
            );
        }
        assert_eq!(
            call(&json!({"name":"dish_preferences","arguments":{"extra":1}}))["isError"],
            true
        );
        assert_eq!(call(&json!({"name":"dish_write"}))["isError"], true);
        assert!(
            dispatch(json!({"jsonrpc":"1.0","id":1,"method":"initialize"}))
                .unwrap()
                .get("error")
                .is_some()
        );
    }
}
