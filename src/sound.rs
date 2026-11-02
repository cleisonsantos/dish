//! Som nativo do sistema ao concluir um turno em segundo plano.
//!
//! Nenhum asset é empacotado: o Dish pede o som ao sistema (no Linux,
//! `pw-play`, `paplay` ou `canberra-gtk-play`; no Windows, `MessageBeep`).
//! Toca só quando um turno com conteúdo termina numa sessão que o usuário não
//! está vendo, uma vez por conclusão. Nesta fase erro e cancelamento não têm
//! som distinto do sucesso.

use std::sync::atomic::{AtomicBool, Ordering};

/// Pure decision: should a completion sound play this frame?
pub fn should_play(enabled: bool, newly_completed: bool, unread_completion: bool) -> bool {
    enabled && newly_completed && unread_completion
}

/// Fire-and-forget: plays on a dedicated thread, never blocks the caller, never panics.
pub fn play() {
    let override_cmd = std::env::var_os("DISH_SOUND_CMD").filter(|v| !v.is_empty());
    let spawned = std::thread::Builder::new().name("dish-sound".into()).spawn(move || {
        if let Err(error) = play_blocking(override_cmd.as_deref()) { warn_once(&error); }
    });
    if let Err(error) = spawned { warn_once(&anyhow::Error::from(error)); }
}

fn play_blocking(override_cmd: Option<&std::ffi::OsStr>) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Some(cmd) = override_cmd {
        let status = std::process::Command::new(cmd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("falha ao executar {}", cmd.to_string_lossy()))?;
        if !status.success() {
            anyhow::bail!("{} terminou com {status}", cmd.to_string_lossy());
        }
        return Ok(());
    }
    play_native()
}

#[cfg(target_os = "linux")]
const LINUX_SOUND_FILE: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

/// Candidatos na ordem de preferência; ausência do arquivo elimina os tocadores
/// que dependem dele.
#[cfg(target_os = "linux")]
fn linux_candidates(sound_file_exists: bool) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut candidates = Vec::new();
    if sound_file_exists {
        candidates.push(("pw-play", vec![LINUX_SOUND_FILE]));
        candidates.push(("paplay", vec![LINUX_SOUND_FILE]));
    }
    candidates.push(("canberra-gtk-play", vec!["-i", "complete"]));
    candidates
}

#[cfg(target_os = "linux")]
fn play_native() -> anyhow::Result<()> {
    let sound_file_exists = std::path::Path::new(LINUX_SOUND_FILE).exists();
    let mut failures = Vec::new();
    for (program, args) in linux_candidates(sound_file_exists) {
        let status = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("{program}: {status}")),
            // Programa ausente: tenta o próximo candidato.
            Err(error) => failures.push(format!("{program}: {error}")),
        }
    }
    anyhow::bail!(
        "nenhum reprodutor de som disponível ({})",
        failures.join("; ")
    )
}

#[cfg(windows)]
fn play_native() -> anyhow::Result<()> {
    use anyhow::Context;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBeep, MB_ICONASTERISK};
    // SAFETY: MessageBeep só toca um som do sistema; sem ponteiros ou estado.
    unsafe { MessageBeep(MB_ICONASTERISK) }.context("falha ao tocar o som do sistema")?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn play_native() -> anyhow::Result<()> {
    Ok(())
}

/// Um aviso por execução: falhar ao tocar som não pode virar ruído contínuo.
fn warn_once(error: &anyhow::Error) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if WARNED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::logging::warn(format_args!("dish: som de conclusão indisponível: {error:#}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_unread_new_completions_play() {
        for enabled in [false, true] {
            for newly_completed in [false, true] {
                for unread_completion in [false, true] {
                    let expected = enabled && newly_completed && unread_completion;
                    assert_eq!(
                        should_play(enabled, newly_completed, unread_completion),
                        expected,
                        "enabled={enabled} newly_completed={newly_completed} unread={unread_completion}"
                    );
                }
            }
        }
        // Casos nomeados: cada um sozinho já impede o som.
        assert!(!should_play(false, true, true), "desligado nunca toca");
        assert!(
            !should_play(true, false, true),
            "quadros seguintes do mesmo turno nunca tocam"
        );
        assert!(
            !should_play(true, true, false),
            "sessão à vista nunca toca"
        );
    }

    #[cfg(unix)]
    fn temp_dir(label: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "dish-sound-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[cfg(unix)]
    fn write_script(
        directory: &std::path::Path,
        name: &str,
        body: &str,
    ) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn missing_or_failing_player_is_an_error() {
        let directory = temp_dir("errors");
        let missing = directory.join("nao-existe");
        assert!(play_blocking(Some(missing.as_os_str())).is_err());
        let failing = write_script(&directory, "falha", "#!/bin/sh\nexit 1\n");
        assert!(play_blocking(Some(failing.as_os_str())).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn successful_player_runs_once() {
        let directory = temp_dir("success");
        let log = directory.join("sound.log");
        let script = write_script(
            &directory,
            "toca",
            &format!("#!/bin/sh\nprintf 'x\\n' >> '{}'\n", log.display()),
        );
        play_blocking(Some(script.as_os_str())).unwrap();
        let lines = std::fs::read_to_string(&log).unwrap();
        assert_eq!(lines.lines().count(), 1, "o tocador roda exatamente uma vez");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repeated_warnings_never_panic() {
        let error = anyhow::anyhow!("falha sintética");
        warn_once(&error);
        warn_once(&error);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_candidates_prefer_the_sound_file() {
        let without_file = linux_candidates(false);
        assert_eq!(without_file.len(), 1);
        assert_eq!(without_file[0].0, "canberra-gtk-play");
        let with_file = linux_candidates(true);
        assert_eq!(
            with_file.iter().map(|(program, _)| *program).collect::<Vec<_>>(),
            vec!["pw-play", "paplay", "canberra-gtk-play"]
        );
        assert_eq!(with_file[0].1, vec![LINUX_SOUND_FILE]);
        assert_eq!(with_file[1].1, vec![LINUX_SOUND_FILE]);
        assert_eq!(with_file[2].1, vec!["-i", "complete"]);
    }

    #[test]
    fn completion_rules_match_the_sound_decision() {
        use crate::session_activity::ResponseTracker;
        use serde_json::json;

        // Turno com texto: conta uma vez; agent_end repetido não conta de novo.
        let mut tracker = ResponseTracker::default();
        tracker.observe("agent_start", &json!({}));
        tracker.observe(
            "message_end",
            &json!({"message":{"role":"assistant","content":[{"type":"text","text":"resposta"}]}}),
        );
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 1, "um som por turno concluído");
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 1, "agent_end duplicado não toca de novo");

        // Erro sem conteúdo: nenhum som.
        let mut tracker = ResponseTracker::default();
        tracker.observe("agent_start", &json!({}));
        tracker.observe(
            "message_end",
            &json!({"message":{"role":"assistant","stopReason":"error","errorMessage":"boom","content":[]}}),
        );
        tracker.observe("agent_end", &json!({}));
        assert_eq!(tracker.completed, 0, "erro sem conteúdo não toca som");

        // Cancelamento com texto parcial: mesmo som do sucesso nesta fase.
        let mut tracker = ResponseTracker::default();
        tracker.observe("agent_start", &json!({}));
        tracker.observe(
            "message_end",
            &json!({"message":{"role":"assistant","stopReason":"aborted","content":[{"type":"text","text":"parcial"}]}}),
        );
        tracker.observe("agent_end", &json!({}));
        assert_eq!(
            tracker.completed, 1,
            "cancelamento com texto usa o mesmo som do sucesso"
        );
    }
}
