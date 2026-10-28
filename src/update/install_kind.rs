//! Como o Dish foi instalado, para decidir se ele pode se atualizar.
//!
//! A detecção é pura: `classify` só olha para os caminhos recebidos e não lê
//! variáveis de ambiente. Quem monta `Detect` a partir do processo atual é uma
//! etapa posterior do auto-update (#138).

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detect {
    pub os: Os,
    /// Valor de `$APPIMAGE`, quando existir. A AppImage é o próprio executável.
    pub appimage: Option<PathBuf>,
    pub exe: PathBuf,
    pub home: PathBuf,
    pub local_app_data: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// Executada a partir de uma AppImage; o arquivo pode ser substituído.
    AppImage,
    /// Instalação solta do usuário (`~/.local/bin` ou uma pasta qualquer no Windows).
    Portable,
    /// Instalada pelo gerenciador de pacotes; atualizada por ele, não pelo Dish.
    SystemPackage,
    /// Instalador NSIS por usuário em `%LOCALAPPDATA%\Programs\Dish`.
    WindowsSetup,
    Unknown,
}

fn non_empty(path: Option<&Path>) -> bool {
    path.is_some_and(|path| !path.as_os_str().is_empty())
}

fn under(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// Compara caminhos do Windows no teste mesmo quando o host é Linux: ali a
/// `Path` não trata `\\` como separador nem ignora maiúsculas.
fn under_windows(path: &Path, root: &Path) -> bool {
    fn normalize(path: &Path) -> String {
        path.to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
    }
    let path = normalize(path);
    let root = normalize(root);
    let root = root.trim_end_matches('/');
    path.as_str() == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Classifica a instalação na ordem documentada pela issue #138.
pub fn classify(d: &Detect) -> InstallKind {
    if non_empty(d.appimage.as_deref()) {
        return InstallKind::AppImage;
    }

    match d.os {
        Os::Windows => match &d.local_app_data {
            Some(root) if under_windows(&d.exe, &root.join("Programs").join("Dish")) => {
                InstallKind::WindowsSetup
            }
            _ => InstallKind::Portable,
        },
        Os::Linux => {
            if under(&d.exe, &d.home.join(".local").join("bin")) {
                InstallKind::Portable
            } else if [
                Path::new("/usr/bin"),
                Path::new("/usr/local/bin"),
                Path::new("/opt"),
            ]
            .iter()
            .any(|root| under(&d.exe, root))
            {
                InstallKind::SystemPackage
            } else {
                InstallKind::Unknown
            }
        }
        Os::Other => InstallKind::Unknown,
    }
}

/// Só instalações que o próprio Dish controla podem trocar o binário.
pub fn can_self_update(kind: InstallKind) -> bool {
    matches!(
        kind,
        InstallKind::AppImage | InstallKind::Portable | InstallKind::WindowsSetup
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/dish";
    const LOCAL_APP_DATA: &str = "C:/Users/dish/AppData/Local";

    fn detect(os: Os, exe: &str) -> Detect {
        Detect {
            os,
            appimage: None,
            exe: PathBuf::from(exe),
            home: PathBuf::from(HOME),
            local_app_data: Some(PathBuf::from(LOCAL_APP_DATA)),
        }
    }

    #[test]
    fn classify_covers_the_five_install_kinds() {
        let mut appimage = detect(Os::Linux, "/usr/bin/dish-pi");
        appimage.appimage = Some(PathBuf::from("/tmp/Dish.AppImage"));

        let mut empty_appimage = detect(Os::Linux, "/home/dish/.local/bin/dish");
        empty_appimage.appimage = Some(PathBuf::new());

        let cases = [
            (
                "AppImage vence o executável do sistema",
                appimage,
                InstallKind::AppImage,
            ),
            (
                "AppImage vence o instalador do Windows",
                {
                    let mut detection = detect(
                        Os::Windows,
                        "C:/Users/dish/AppData/Local/Programs/Dish/dish.exe",
                    );
                    detection.appimage = Some(PathBuf::from("D:/Dish.AppImage"));
                    detection
                },
                InstallKind::AppImage,
            ),
            (
                "APPIMAGE vazio não vence",
                empty_appimage,
                InstallKind::Portable,
            ),
            (
                "Linux solto em ~/.local/bin",
                detect(Os::Linux, "/home/dish/.local/bin/dish"),
                InstallKind::Portable,
            ),
            (
                "deb/rpm em /usr/bin",
                detect(Os::Linux, "/usr/bin/dish-pi"),
                InstallKind::SystemPackage,
            ),
            (
                "instalação local em /usr/local/bin",
                detect(Os::Linux, "/usr/local/bin/dish"),
                InstallKind::SystemPackage,
            ),
            (
                "instalação em /opt",
                detect(Os::Linux, "/opt/dish/dish"),
                InstallKind::SystemPackage,
            ),
            (
                "instalador por usuário no Windows",
                detect(
                    Os::Windows,
                    "C:/Users/dish/AppData/Local/Programs/Dish/dish.exe",
                ),
                InstallKind::WindowsSetup,
            ),
            (
                "instalador no Windows com separador \\",
                detect(
                    Os::Windows,
                    r"C:\Users\dish\AppData\Local\Programs\Dish\dish.exe",
                ),
                InstallKind::WindowsSetup,
            ),
            (
                "Windows fora de Programs/Dish",
                detect(Os::Windows, "C:/Users/dish/Downloads/dish.exe"),
                InstallKind::Portable,
            ),
            (
                "Linux em caminho desconhecido",
                detect(Os::Linux, "/tmp/dish"),
                InstallKind::Unknown,
            ),
            (
                "outro sistema",
                detect(Os::Other, "/usr/bin/dish-pi"),
                InstallKind::Unknown,
            ),
        ];

        for (case, detection, expected) in cases {
            assert_eq!(classify(&detection), expected, "{case}");
        }
    }

    #[test]
    fn only_owned_installs_can_self_update() {
        for (kind, expected) in [
            (InstallKind::AppImage, true),
            (InstallKind::Portable, true),
            (InstallKind::WindowsSetup, true),
            (InstallKind::SystemPackage, false),
            (InstallKind::Unknown, false),
        ] {
            assert_eq!(can_self_update(kind), expected, "{kind:?}");
        }
    }
}
