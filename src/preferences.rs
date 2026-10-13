//! UI preferences, separate from Pi's conversation storage.
use std::{collections::{BTreeMap, BTreeSet}, path::PathBuf, sync::{mpsc, OnceLock}};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub details_open: Option<bool>,
    pub navigation_open: Option<bool>,
    /// Largura do painel de sessões, em pixels.
    pub navigation_width: Option<f32>,
    /// Se o raciocínio começa expandido ou recolhido.
    pub show_thinking: Option<bool>,
    /// Títulos de sessão gerados pelo modelo da sessão. Gasta tokens, então
    /// começa desligado e é escolha explícita do usuário.
    pub generated_titles: bool,
    /// Modelos ocultos no seletor do Dish, como `provider/id`.
    /// O Dish nunca altera a lista de modelos do Pi: isto é só apresentação.
    pub hidden_models: BTreeSet<String>,
    pub collapsed_projects: BTreeSet<PathBuf>,
    pub last_project: Option<PathBuf>,
    pub last_session: Option<PathBuf>,
    pub window_size: Option<[f32; 2]>,
    pub pi_executable: Option<PathBuf>,
    /// Client-observed completion time for responses not yet viewed.
    pub unread_sessions: BTreeMap<PathBuf, u64>,
    pub session_filter: crate::session_activity::SessionFilter,
    /// Aparência escolhida: escuro, claro ou seguindo o desktop.
    pub appearance: crate::theme::AppearanceMode,
    /// Tema fixado pelo usuário, por id (`dish-paper`, `user:meu-tema`…).
    /// `None` usa o tema padrão da aparência resolvida.
    pub theme: Option<String>,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Raiz de configuração: XDG no Unix, `%APPDATA%` no Windows (onde `HOME` e
/// `XDG_CONFIG_HOME` não existem). Sem isso nada era salvo no Windows — nem a
/// escolha do Pi, nem tamanho de janela, nem modelos ocultos.
fn config_root(xdg: Option<PathBuf>, appdata: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg.or(appdata).or_else(|| home.map(|home| home.join(".config")))
}

/// Diretório de configuração do Dish (`~/.config/dish`, `%APPDATA%/dish`).
fn config_dir() -> Option<PathBuf> {
    let appdata = if cfg!(windows) { env_path("APPDATA") } else { None };
    config_root(env_path("XDG_CONFIG_HOME"), appdata, env_path("HOME"))
        .map(|root| root.join("dish"))
}

fn path() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("state.json"))
}

/// Onde ficam os temas do usuário, lidos na inicialização.
pub fn themes_dir() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("themes"))
}

pub fn load() -> Preferences {
    let mut preferences: Preferences = path().and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    if let Some(path) = crate::installation::selected() { preferences.pi_executable = Some(path); }
    preferences
}

// A single writer preserves ordering and keeps disk IO off the UI thread.
pub fn save(preferences: Preferences) {
    static WRITER: OnceLock<mpsc::Sender<Preferences>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Preferences>();
        std::thread::spawn(move || {
            while let Ok(mut preferences) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() { preferences = newer; }
                let Some(path) = path() else { continue; };
                if let Err(error) = write(&path, &preferences) {
                    crate::logging::warn(format_args!("Dish: cannot save UI preferences: {error}"));
                }
            }
        });
        sender
    });
    let _ = writer.send(preferences);
}

fn write(path: &std::path::Path, preferences: &Preferences) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(preferences)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_root_follows_the_platform_convention() {
        let xdg = Some(PathBuf::from("/xdg"));
        let appdata = Some(PathBuf::from("C:/Users/dish/AppData/Roaming"));
        let home = Some(PathBuf::from("/home/dish"));
        assert_eq!(config_root(xdg.clone(), appdata.clone(), home.clone()), xdg);
        assert_eq!(config_root(None, appdata.clone(), home.clone()), appdata);
        assert_eq!(config_root(None, None, home), Some(PathBuf::from("/home/dish/.config")));
        assert_eq!(config_root(None, None, None), None);
    }

    #[test]
    fn old_or_partial_preferences_use_defaults() {
        let preferences: Preferences = serde_json::from_str(r#"{"details_open":false,"unknown":1}"#).unwrap();
        assert_eq!(preferences.details_open, Some(false));
        assert_eq!(preferences.navigation_open, None);
        assert!(preferences.collapsed_projects.is_empty());
        assert_eq!(preferences.appearance, crate::theme::AppearanceMode::System);
        assert_eq!(preferences.theme, None);
    }
    #[test]
    fn atomic_write_creates_directory_and_replaces_file() {
        let root = std::env::temp_dir().join(format!("dish-prefs-test-{}", std::process::id()));
        let path = root.join("config/state.json");
        let first = Preferences::default();
        write(&path, &first).unwrap();
        let second = Preferences { details_open: Some(false), ..first };
        write(&path, &second).unwrap();
        let loaded: Preferences = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded, second);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preferences_round_trip() {
        let preferences = Preferences {
            details_open: Some(false), navigation_open: Some(true),
            navigation_width: Some(264.),
            show_thinking: Some(false),
            generated_titles: true,
            hidden_models: ["anthropic/claude-sonnet-4".to_string()].into(),
            collapsed_projects: [PathBuf::from("/tmp/projeto-á")].into(),
            last_project: Some("/tmp/projeto-á".into()), last_session: Some("/tmp/session.jsonl".into()),
            window_size: Some([1240., 860.]),
            pi_executable: None,
            unread_sessions: [(PathBuf::from("/tmp/session.jsonl"), 123456)].into(),
            session_filter: crate::session_activity::SessionFilter::Unread,
            appearance: crate::theme::AppearanceMode::Dark,
            theme: Some("user:meu-tema".to_string()),
        };
        let bytes = serde_json::to_vec(&preferences).unwrap();
        assert_eq!(serde_json::from_slice::<Preferences>(&bytes).unwrap(), preferences);
    }
}
