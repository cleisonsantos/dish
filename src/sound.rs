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
/// Volume 0 é mudo: não cria thread nem executa processo, inclusive o override.
pub fn play(volume: u8) {
    let volume = volume.min(100);
    if volume == 0 { return; }
    let override_cmd = std::env::var_os("DISH_SOUND_CMD").filter(|v| !v.is_empty());
    let spawned = std::thread::Builder::new().name("dish-sound".into()).spawn(move || {
        if let Err(error) = play_blocking(override_cmd.as_deref(), volume) { warn_once(&error); }
    });
    if let Err(error) = spawned { warn_once(&anyhow::Error::from(error)); }
}

fn play_blocking(override_cmd: Option<&std::ffi::OsStr>, volume: u8) -> anyhow::Result<()> {
    use anyhow::Context;
    // Mudo também nos caminhos síncronos e nos fallbacks sem controle de volume.
    let volume = volume.min(100);
    if volume == 0 { return Ok(()); }
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
    play_native(volume)
}

#[cfg(target_os = "linux")]
const LINUX_SOUND_FILE: &str = "/usr/share/sounds/freedesktop/stereo/complete.oga";

/// Candidatos na ordem de preferência; ausência do arquivo elimina os tocadores
/// que dependem dele.
#[cfg(target_os = "linux")]
fn linux_candidates(sound_file_exists: bool, volume: u8) -> Vec<(&'static str, Vec<String>)> {
    let volume = volume.min(100);
    let mut candidates = Vec::new();
    if sound_file_exists {
        candidates.push(("pw-play", vec!["--volume".into(), format!("{:.2}", f64::from(volume) / 100.), LINUX_SOUND_FILE.into()]));
        candidates.push(("paplay", vec!["--volume".into(), ((u32::from(volume) * 65536 + 50) / 100).to_string(), LINUX_SOUND_FILE.into()]));
    }
    // Fallback mantido sem ajuste de volume nesta tarefa. A opção --volume
    // do canberra-gtk-play usa dB, não percentual ou a escala dos outros tocadores.
    candidates.push(("canberra-gtk-play", vec!["-i".into(), "complete".into()]));
    candidates
}

#[cfg(target_os = "linux")]
fn play_native(volume: u8) -> anyhow::Result<()> {
    let sound_file_exists = std::path::Path::new(LINUX_SOUND_FILE).exists();
    let mut failures = Vec::new();
    for (program, args) in linux_candidates(sound_file_exists, volume) {
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
fn play_native(_volume: u8) -> anyhow::Result<()> {
    use anyhow::Context;
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
    // MessageBeep não oferece volume por chamada; usa o volume do sistema.
    // SAFETY: MessageBeep só toca um som do sistema; sem ponteiros ou estado.
    unsafe { MessageBeep(MB_ICONASTERISK) }.context("falha ao tocar o som do sistema")?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn play_native(_volume: u8) -> anyhow::Result<()> {
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

    /// Executar um script recém-escrito pode falhar com `ETXTBSY`: outro teste do
    /// mesmo binário pode ter feito `fork` enquanto o arquivo ainda estava aberto
    /// para escrita, e o filho segura o descritor até o `exec`. Repetimos algumas
    /// vezes — só esse erro — em vez de tratar a corrida como falha do tocador.
    #[cfg(unix)]
    fn text_file_busy(error: &anyhow::Error) -> bool {
        const ETXTBSY: i32 = 26;
        error
            .chain()
            .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
            .any(|io_error| io_error.raw_os_error() == Some(ETXTBSY))
    }

    #[cfg(unix)]
    fn play_written_script(path: &std::ffi::OsStr) -> anyhow::Result<()> {
        const ATTEMPTS: usize = 50;
        let mut attempt = 0;
        loop {
            attempt += 1;
            match play_blocking(Some(path), 35) {
                Err(error) if attempt < ATTEMPTS && text_file_busy(&error) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn missing_or_failing_player_is_an_error() {
        let directory = temp_dir("errors");
        let missing = directory.join("nao-existe");
        assert!(play_blocking(Some(missing.as_os_str()), 35).is_err());
        let failing = write_script(&directory, "falha", "#!/bin/sh\nexit 1\n");
        assert!(play_written_script(failing.as_os_str()).is_err());
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
        play_written_script(script.as_os_str()).unwrap();
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
        let without_file = linux_candidates(false, 35);
        assert_eq!(without_file.len(), 1);
        assert_eq!(without_file[0].0, "canberra-gtk-play");
        let with_file = linux_candidates(true, 35);
        assert_eq!(
            with_file.iter().map(|(program, _)| *program).collect::<Vec<_>>(),
            vec!["pw-play", "paplay", "canberra-gtk-play"]
        );
        assert_eq!(with_file[0].1, vec!["--volume", "0.35", LINUX_SOUND_FILE]);
        assert_eq!(with_file[1].1, vec!["--volume", "22938", LINUX_SOUND_FILE]);
        assert_eq!(with_file[2].1, vec!["-i", "complete"]);
    }

    #[cfg(unix)]
    #[test]
    fn zero_volume_never_executes_a_player() {
        let directory = temp_dir("muted");
        let log = directory.join("sound.log");
        let script = write_script(&directory, "toca", &format!(
            "#!/bin/sh\nprintf 'x' > '{}'\n", log.display(),
        ));
        play_blocking(Some(script.as_os_str()), 0).unwrap();
        play(0);
        assert!(!log.exists(), "mudo não executa nem o comando de override");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_players_receive_volume_arguments() {
        let directory = temp_dir("volume-args");
        let log = directory.join("args.log");
        for (program, args) in linux_candidates(true, 35).into_iter().take(2) {
            write_script(&directory, program, &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", log.display(),
            ));
            // PATH só do filho: não altera o ambiente dos testes concorrentes.
            let status = std::process::Command::new("/bin/sh")
                .args(["-c", "exec \"$@\"", "dish-sound-test", program])
                .args(&args)
                .env("PATH", &directory)
                .status().unwrap();
            assert!(status.success());
            let expected = if program == "pw-play" { "0.35" } else { "22938" };
            assert_eq!(std::fs::read_to_string(&log).unwrap(),
                format!("--volume\n{expected}\n{LINUX_SOUND_FILE}\n"));
        }
        for (volume, pw, pa) in [(0, "0.00", "0"), (100, "1.00", "65536"), (255, "1.00", "65536")] {
            let candidates = linux_candidates(true, volume);
            assert_eq!(candidates[0].1[1], pw);
            assert_eq!(candidates[1].1[1], pa);
        }
        std::fs::remove_dir_all(directory).unwrap();
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
