//! Ejecución de procesos hijo (yt-dlp, ffmpeg, ffprobe).
//!
//! Tres reglas que valen para todo lo que se lanza desde acá:
//!
//! 1. **Los argumentos van como array, nunca como string de shell.** No se
//!    invoca `cmd.exe` ni `sh`, así que una URL con comillas, `&&` o `;` es
//!    apenas un argumento raro y no un comando.
//! 2. **Todo tiene timeout.** Un yt-dlp colgado contra Instagram no puede
//!    quedarse esperando para siempre en la máquina de alguien que está en vivo.
//! 3. **Prioridad below-normal.** El streamer está transcodificando en OBS y
//!    probablemente jugando; esto no puede pelearle CPU.

use crate::error::{ErrorCode, ErrorDetail, Result};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Windows: BELOW_NORMAL_PRIORITY_CLASS.
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
/// Windows: CREATE_NO_WINDOW, para que no parpadee una consola sobre el stream.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Output {
    pub status_ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Corre un programa con argumentos y espera a que termine, con timeout.
///
/// Si vence el timeout, el hijo se mata antes de devolver: dejarlo suelto
/// significaría un yt-dlp zombi comiendo red y CPU durante todo el stream.
pub async fn run<S: AsRef<OsStr>>(
    program: &Path,
    args: &[S],
    timeout: Duration,
    what: &str,
) -> Result<Output> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(windows)]
    cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);

    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ErrorDetail::new(
                ErrorCode::BinaryMissing,
                format!("No se encontró {}: {}", what, program.display()),
            )
        } else {
            ErrorDetail::new(
                ErrorCode::BinaryMissing,
                format!("No se pudo ejecutar {}: {e}", what),
            )
        }
    })?;

    let out = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return Err(ErrorDetail::new(
                ErrorCode::DownloadFailed,
                format!("Falló {}: {e}", what),
            ))
        }
        Err(_) => {
            // `kill_on_drop` mata al hijo cuando se descarta el future.
            return Err(ErrorDetail::new(
                ErrorCode::Timeout,
                format!("{} tardó más de {}s y se canceló.", what, timeout.as_secs()),
            ));
        }
    };

    Ok(Output {
        status_ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}


/// Argumentos que se le pasan a un proceso, para logs y tests.
///
/// Nunca se concatena en un string para ejecutar: esto es SOLO para mostrar.
pub fn render_args<S: AsRef<OsStr>>(program: &Path, args: &[S]) -> String {
    let mut out = program.display().to_string();
    for a in args {
        out.push(' ');
        let s = a.as_ref().to_string_lossy();
        if s.contains(' ') {
            out.push('"');
            out.push_str(&s);
            out.push('"');
        } else {
            out.push_str(&s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_es_solo_para_mostrar() {
        let p = PathBuf::from("yt-dlp");
        let s = render_args(&p, &["--cookies", "C:\\ruta con espacios\\cookies.txt"]);
        assert!(s.contains("\"C:\\ruta con espacios\\cookies.txt\""));
    }

    #[tokio::test]
    async fn un_binario_inexistente_da_binary_missing() {
        let p = PathBuf::from("no-existe-este-programa-12345");
        let r = run(&p, &["--version"], Duration::from_secs(5), "prueba").await;
        match r {
            Err(d) => assert_eq!(d.code, ErrorCode::BinaryMissing),
            Ok(_) => panic!("no debería existir ese binario"),
        }
    }
}
