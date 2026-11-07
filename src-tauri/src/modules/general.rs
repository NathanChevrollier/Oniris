//! Exécutions réelles des outils généraux d'IRIS (jamais appelées par les tests hors dossier temporaire : R8).
use async_trait::async_trait;
use base64::Engine;
use oniris_core::general::shell::{ShellExec, ShellOutput, MAX_OUTPUT};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// Sortie en UTF-8 et sans barre de progression (qui irait sur stderr).
const PRELUDE: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;$ProgressPreference='SilentlyContinue';";

pub struct RealShell;

/// Garde au plus `MAX_OUTPUT` octets puis vide le tube (le processus ne doit jamais bloquer sur un tube plein).
async fn read_capped(mut r: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = (&mut r).take(MAX_OUTPUT as u64).read_to_end(&mut buf).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    buf
}

#[async_trait]
impl ShellExec for RealShell {
    async fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<ShellOutput, String> {
        let utf16: Vec<u8> = format!("{PRELUDE}{command}").encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        let root = std::env::var_os("SystemRoot").ok_or("SystemRoot absente")?;
        let exe = Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut cmd = Command::new(exe);
        cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .current_dir(cwd)
            .env("GIT_PAGER", "cat")
            .env("PAGER", "")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, job) = crate::job::spawn(cmd).map_err(|e| format!("lancement impossible : {e}"))?;
        let (out, err) = (child.stdout.take().ok_or("stdout")?, child.stderr.take().ok_or("stderr")?);
        let work = async { tokio::join!(child.wait(), read_capped(out), read_capped(err)) };
        match tokio::time::timeout(timeout, work).await {
            Ok((status, mut o, e)) => {
                o.extend(e);
                let mut text = String::from_utf8_lossy(&o).into_owned();
                if text.len() > MAX_OUTPUT {
                    let cut = (0..=MAX_OUTPUT).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
                    text.truncate(cut);
                }
                drop(job); // tue d'éventuels descendants restés vivants
                Ok(ShellOutput { exit_code: status.map_err(|e| e.to_string())?.code(), output: text, timed_out: false })
            }
            Err(_) => {
                drop(job);
                Ok(ShellOutput { exit_code: None, output: String::new(), timed_out: true })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    async fn run(cmd: &str, secs: u64) -> ShellOutput {
        let t = tempfile::tempdir().unwrap();
        RealShell.run(cmd, t.path(), Duration::from_secs(secs)).await.unwrap()
    }

    #[tokio::test]
    async fn rend_l_unicode() {
        let o = run("Write-Output 'é'", 30).await;
        assert_eq!(o.output.trim(), "é");
        assert_eq!(o.exit_code, Some(0));
    }

    #[tokio::test]
    async fn delai_depasse() {
        let t0 = Instant::now();
        let o = run("Start-Sleep 5", 1).await;
        assert!(o.timed_out);
        assert!(t0.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn sortie_tronquee() {
        let o = run("Write-Output ('a' * 100000)", 30).await;
        assert_eq!(o.output.len(), MAX_OUTPUT);
    }
}
