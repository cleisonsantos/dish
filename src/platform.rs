//! Pequenas diferenças entre sistemas operacionais.

/// No Windows, impede que um processo filho abra a própria janela de console.
/// Sem isso, `pi` (e outros filhos) aparecem como um terminal flutuante ao lado
/// da interface. Nos demais sistemas não faz nada.
pub fn hide_console(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: vale também em builds de debug, onde o Dish roda
        // a partir de um terminal e não quer uma janela extra por filho.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// Abre uma pasta no gerenciador de arquivos do sistema. Fire-and-forget:
/// falhar em abrir não pode derrubar a interface.
pub fn open_directory(directory: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let mut command = std::process::Command::new(program);
    command.arg(directory);
    hide_console(&mut command);
    if let Err(error) = command.spawn() {
        crate::logging::warn(format_args!(
            "Dish: cannot open {program} for {}: {error}",
            directory.display()
        ));
    }
}

/// Abre um endereço no navegador padrão. Usado pelo login OAuth, onde o Pi
/// conduz a autorização e só precisa que a página apareça.
///
/// Fire-and-forget pelo mesmo motivo de `open_directory`, e restrito a https:
/// um endereço vindo de um provedor não deve conseguir abrir outra coisa.
pub fn open_url(url: &str) {
    if !url.starts_with("https://") {
        crate::logging::warn(format_args!("Dish: refusing to open non-https url"));
        return;
    }
    #[cfg(target_os = "windows")]
    let program = "rundll32";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let mut command = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    command.arg("url.dll,FileProtocolHandler");
    command.arg(url);
    hide_console(&mut command);
    if let Err(error) = command.spawn() {
        crate::logging::warn(format_args!("Dish: cannot open {program}: {error}"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hiding_the_console_is_safe_on_any_platform() {
        let mut command = std::process::Command::new("true");
        super::hide_console(&mut command);
        // O comando continua utilizável depois de receber (ou não) as flags.
        assert!(command.get_program().to_string_lossy().contains("true"));
    }
}
