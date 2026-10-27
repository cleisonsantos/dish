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
    /// Pastas usadas recentemente, da mais recente para a mais antiga.
    /// Limitada: é atalho de navegação, não histórico.
    pub recent_projects: Vec<PathBuf>,
    /// Sessões fixadas no topo do grupo da pasta, por caminho do arquivo.
    /// Só apresentação do Dish: os arquivos do Pi não mudam.
    pub pinned_sessions: BTreeSet<PathBuf>,
    pub window_size: Option<[f32; 2]>,
    pub pi_executable: Option<PathBuf>,
    /// Client-observed completion time for responses not yet viewed.
    pub unread_sessions: BTreeMap<PathBuf, u64>,
    pub session_filter: crate::session_activity::SessionFilter,
    /// Aparência escolhida: escuro, claro ou seguindo o desktop.
    pub appearance: crate::theme::AppearanceMode,
    /// `None` preserves GPUI's platform preference; `Some` is an explicit override.
    pub reduce_motion: Option<bool>,
    /// Tema fixado pelo usuário, por id (`dish-paper`, `user:meu-tema`…).
    /// `None` usa o tema padrão da aparência resolvida.
    pub theme: Option<String>,
    /// Stable notification ids the user has dismissed.
    pub dismissed_notifications: BTreeSet<String>,
    /// At most one automatic Pi update check per day. Off by default: Dish is
    /// not Pi's package manager.
    pub pi_auto_check: bool,
    /// Only meaningful when `pi_auto_check` is on. Explicit opt-in to running
    /// the official installer after a check finds a newer version.
    pub pi_auto_install: bool,
    /// Unix seconds of the last Pi update attempt, including failures.
    pub last_pi_update_check: Option<i64>,
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

impl Preferences {
    /// Põe a pasta no topo dos recentes, sem repetir e com um teto.
    pub fn remember_project(&mut self, path: &std::path::Path) {
        const LIMIT: usize = 8;
        self.recent_projects.retain(|item| item != path);
        self.recent_projects.insert(0, path.to_path_buf());
        self.recent_projects.truncate(LIMIT);
    }
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
    let bytes = serde_json::to_vec_pretty(preferences)?;
    crate::fs_security::write_atomic(path, &bytes)?;
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
        assert_eq!(preferences.reduce_motion, None, "preserve platform motion setting");
        assert!(preferences.dismissed_notifications.is_empty());
        assert!(!preferences.pi_auto_check, "automatic Pi checks are opt-in");
        assert!(!preferences.pi_auto_install, "automatic Pi installs are opt-in");
        assert_eq!(preferences.last_pi_update_check, None);
    }

    #[test]
    fn motion_preference_preserves_explicit_choices() {
        for reduced in [false, true] {
            let preferences = Preferences { reduce_motion: Some(reduced), ..Preferences::default() };
            let json = serde_json::to_string(&preferences).unwrap();
            let restored: Preferences = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.reduce_motion, Some(reduced));
        }
    }
    #[test]
    fn remember_project_moves_to_front_dedupes_and_caps() {
        let mut preferences = Preferences::default();
        for index in 0..10 {
            preferences.remember_project(&PathBuf::from(format!("/tmp/projeto-{index}")));
        }
        assert_eq!(preferences.recent_projects.len(), 8, "teto de 8");
        assert_eq!(preferences.recent_projects[0], PathBuf::from("/tmp/projeto-9"));
        preferences.remember_project(&PathBuf::from("/tmp/projeto-5"));
        assert_eq!(preferences.recent_projects[0], PathBuf::from("/tmp/projeto-5"));
        assert_eq!(
            preferences.recent_projects.iter().filter(|path| *path == &PathBuf::from("/tmp/projeto-5")).count(),
            1,
            "sem repetir"
        );
        assert_eq!(preferences.recent_projects.len(), 8);
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
            recent_projects: [PathBuf::from("/tmp/projeto-á"), PathBuf::from("/tmp/outro")].into(),
            pinned_sessions: [PathBuf::from("/tmp/session.jsonl")].into(),
            window_size: Some([1240., 860.]),
            pi_executable: None,
            unread_sessions: [(PathBuf::from("/tmp/session.jsonl"), 123456)].into(),
            session_filter: crate::session_activity::SessionFilter::Unread,
            appearance: crate::theme::AppearanceMode::Dark,
            reduce_motion: Some(true),
            theme: Some("user:meu-tema".to_string()),
            dismissed_notifications: ["sample".to_string()].into(),
            pi_auto_check: true,
            pi_auto_install: false,
            last_pi_update_check: Some(1_700_000_000),
        };
        let bytes = serde_json::to_vec(&preferences).unwrap();
        assert_eq!(serde_json::from_slice::<Preferences>(&bytes).unwrap(), preferences);
    }
}
