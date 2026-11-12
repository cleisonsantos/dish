//! Registro explícito e conservador no mcp.json do Pi.
use anyhow::{bail, Context};
use serde_json::{json, Value};
use std::{fs, path::Path};

pub type Error = anyhow::Error;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Changed,
    Unchanged,
}

pub fn install(pi_config: &Path) -> Result<Outcome, Error> {
    edit(pi_config, true)
}

pub fn uninstall(pi_config: &Path) -> Result<Outcome, Error> {
    edit(pi_config, false)
}

fn edit(path: &Path, install: bool) -> Result<Outcome, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => bail!("Não foi possível ler mcp.json."),
    };
    let existing = bytes
        .as_deref()
        .map(serde_json::from_slice::<Value>)
        .transpose()
        .context("mcp.json contém JSON inválido.")?;
    let executable = std::env::current_exe().context("Não foi possível localizar o Dish.")?;
    let Some(updated) = plan_mcp_edit(existing.as_ref(), install, &executable)? else {
        return Ok(Outcome::Unchanged);
    };
    let output = serde_json::to_vec_pretty(&updated)?;
    crate::fs_security::write_atomic(path, &output).context("Não foi possível gravar mcp.json.")?;
    Ok(Outcome::Changed)
}

fn plan_mcp_edit(
    existing: Option<&Value>,
    install: bool,
    executable: &Path,
) -> Result<Option<Value>, Error> {
    let mut document = existing
        .cloned()
        .unwrap_or_else(|| json!({"mcpServers": {}}));
    let root = document
        .as_object_mut()
        .context("A raiz de mcp.json precisa ser um objeto.")?;
    if !root.contains_key("mcpServers") {
        if !install {
            return Ok(None);
        }
        root.insert("mcpServers".into(), json!({}));
    }
    let servers = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .context("mcpServers precisa ser um objeto.")?;
    if let Some(entry) = servers.get("dish") {
        if entry.get("x-dish-managed") != Some(&json!("176")) {
            if !install {
                return Ok(None);
            }
            bail!("Existe uma entrada dish que o Dish não criou.");
        }
    }
    if install {
        servers.insert(
            "dish".into(),
            json!({
                "command": executable, "args": ["--mcp-serve"], "timeout": 180,
                "description": "Lê estado, sessões e preferências do Dish.",
                "x-dish-managed": "176"
            }),
        );
    } else if servers.remove("dish").is_none() {
        return Ok(None);
    }
    Ok((existing != Some(&document)).then_some(document))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_preserves_foreign_entries_and_rejects_bad_documents() {
        let exe = Path::new("/tmp/dish");
        for value in [
            json!(null),
            json!([]),
            json!({"mcpServers": []}),
            json!({"mcpServers":{"dish":{"command":"other"}}}),
        ] {
            assert!(plan_mcp_edit(Some(&value), true, exe).is_err());
        }
        let foreign = json!({"extra": [1], "mcpServers": {"other": {"command":"other"}}});
        let installed = plan_mcp_edit(Some(&foreign), true, exe).unwrap().unwrap();
        assert_eq!(installed["extra"], foreign["extra"]);
        assert_eq!(
            installed["mcpServers"]["other"],
            foreign["mcpServers"]["other"]
        );
        assert!(plan_mcp_edit(Some(&installed), true, exe)
            .unwrap()
            .is_none());
        assert_eq!(
            plan_mcp_edit(Some(&installed), false, exe).unwrap(),
            Some(foreign)
        );
        assert!(plan_mcp_edit(None, false, exe).unwrap().is_none());
    }

    #[test]
    fn registry_is_byte_idempotent_and_never_touches_unmanaged_or_invalid_files() {
        let root = std::env::temp_dir().join(format!("dish-mcp-registry-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("mcp.json");
        let _ = fs::remove_file(&path);
        assert_eq!(install(&path).unwrap(), Outcome::Changed);
        let bytes = fs::read(&path).unwrap();
        assert_eq!(install(&path).unwrap(), Outcome::Unchanged);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(uninstall(&path).unwrap(), Outcome::Changed);
        assert_eq!(uninstall(&path).unwrap(), Outcome::Unchanged);
        for bytes in [
            b"{invalid".as_slice(),
            b"{ \"mcpServers\": {\"dish\": {\"command\": \"other\"}} }\n",
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(install(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let bytes = b"{ \"mcpServers\": {\"dish\": {\"command\": \"other\"}} }\n";
        fs::write(&path, bytes).unwrap();
        assert_eq!(uninstall(&path).unwrap(), Outcome::Unchanged);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }
}
