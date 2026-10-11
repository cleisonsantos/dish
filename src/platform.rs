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
