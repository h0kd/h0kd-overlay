//! Núcleo de Video Requests: metadata, descarga, validación y recodificación.
//!
//! **Sin dependencias de Tauri, a propósito.** Todo lo que hay acá se puede
//! correr desde una terminal con `vrc-cli`, que es donde vive el riesgo real de
//! este módulo: la extracción de Instagram se rompe sola cada tanto, y hace
//! falta poder probarla contra Reels de verdad sin levantar la app, sin OBS y
//! sin estar en vivo.
//!
//! Reglas que se sostienen en todo el crate:
//!
//! - **Cero `panic!`, `unwrap()` y `expect()`.** El binario final se compila con
//!   `panic = "abort"`: un panic acá no es un error manejable, es la app entera
//!   cerrándose a mitad de stream.
//! - **Los argumentos de los procesos van como array**, nunca como string de
//!   shell. No se invoca `cmd.exe` ni `sh` en ningún lado.
//! - **Los nombres de archivo los elegimos nosotros** (el `item_id`), nunca el
//!   título remoto.
//! - **Todo proceso tiene timeout** y corre en prioridad below-normal, porque
//!   del otro lado hay alguien transmitiendo.

pub mod allowlist;
pub mod binaries;
pub mod cookies;
pub mod error;
pub mod ffmpeg;
pub mod pipeline;
pub mod proc;
pub mod ytdlp;

pub use allowlist::Platform;
pub use binaries::{Binaries, BinaryStatus};
pub use cookies::{CookieState, CookieStatus};
pub use error::{ErrorCode, ErrorDetail, Result};
pub use ffmpeg::{Encoder, VideoInfo};
pub use pipeline::{PipelineConfig, Prepared};
pub use ytdlp::Metadata;

/// Diagnóstico completo del entorno, para la UI del agente y para `vrc-cli doctor`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Doctor {
    pub binaries: BinaryStatus,
    pub ytdlp_version: Option<String>,
    pub ffmpeg_found: bool,
    pub ffprobe_found: bool,
    pub cookies: CookieStatus,
    pub cookies_look_valid: bool,
}

/// Revisa qué hay y qué falta para que el módulo pueda funcionar.
///
/// Un chequeo que se pueda correr ANTES de estar en vivo vale más que un
/// mensaje de error preciso a mitad de stream.
pub async fn doctor(data_dir: &std::path::Path) -> Doctor {
    let bins = binaries::resolve(data_dir);
    let status = binaries::status(data_dir);

    let ytdlp_version = ytdlp::version(&bins.ytdlp).await;

    // Se comprueba ejecutándolos, no mirando si el archivo existe: un binario
    // en el PATH no aparece en `status`, y uno corrupto sí aparece.
    let ffmpeg_found = probe_runs(&bins.ffmpeg).await;
    let ffprobe_found = probe_runs(&bins.ffprobe).await;

    let cookie_status = cookies::read_status(data_dir);
    let cookies_look_valid = cookie_status.present
        && cookies::looks_like_netscape(&cookies::cookies_path(data_dir));

    Doctor {
        binaries: status,
        ytdlp_version,
        ffmpeg_found,
        ffprobe_found,
        cookies: cookie_status,
        cookies_look_valid,
    }
}

async fn probe_runs(path: &std::path::Path) -> bool {
    matches!(
        proc::run(path, &["-version"], std::time::Duration::from_secs(20), "binario").await,
        Ok(out) if out.status_ok
    )
}
