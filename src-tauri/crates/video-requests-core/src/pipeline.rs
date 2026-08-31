//! El pipeline completo de un pedido.
//!
//! Orden fijo, y cada paso existe por un motivo:
//!
//! ```text
//!   allowlist  →  el agente decide qué dominios toca, no se lo delega al DO
//!   metadata   →  duración y título SIN bajar un byte
//!   descarga   →  solo después de que un humano aprobó
//!   ffprobe    →  ¿esto es realmente un video?
//!   recodificar → uniformidad, saneamiento y corte duro de duración
//! ```
//!
//! Nada de esto hace `panic!`: todo error viaja como `ErrorDetail` con un
//! código accionable y un mensaje que se le puede mostrar a un mod.

use crate::allowlist::{self, Platform};
use crate::binaries::Binaries;
use crate::error::Result;
use crate::ffmpeg::{self, Encoder, VideoInfo};
use crate::ytdlp::{self, Metadata};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub max_duration_seconds: u32,
    pub max_filesize_mb: u32,
    /// Lado corto de la salida. 720 por defecto.
    pub max_short_side: u32,
    /// Lado largo de la salida. 1280 por defecto.
    pub max_long_side: u32,
    pub encoder: Encoder,
    pub cookies: Option<PathBuf>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        PipelineConfig {
            max_duration_seconds: 30,
            max_filesize_mb: 100,
            max_short_side: 720,
            max_long_side: 1280,
            encoder: Encoder::Nvenc,
            cookies: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub item_id: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    pub encoder_used: Encoder,
}

/// Paso 1: metadata sin descargar, con el chequeo de duración.
///
/// Devuelve la metadata aunque el video sea muy largo — quien llama necesita la
/// duración para explicar el rechazo. La decisión de estado la toma la nube.
pub async fn fetch_metadata(
    bins: &Binaries,
    url: &str,
    cfg: &PipelineConfig,
) -> Result<(Platform, Metadata)> {
    let platform = allowlist::check(url)?;
    let cookies = cfg.cookies.as_deref();
    let meta = ytdlp::fetch_metadata(&bins.ytdlp, url, platform, cookies).await?;
    Ok((platform, meta))
}

/// Paso 2 en adelante: descargar, validar y recodificar.
///
/// Solo se llama después de que un mod aprobó. El archivo final queda en
/// `dest_dir/<item_id>.mp4`, con el nombre que elegimos nosotros.
pub async fn prepare(
    bins: &Binaries,
    url: &str,
    item_id: &str,
    dest_dir: &Path,
    cfg: &PipelineConfig,
) -> Result<Prepared> {
    // Se revalida el dominio aunque ya lo hayan validado dos veces: es el
    // último punto antes de ejecutar un proceso sobre esa URL.
    let platform = allowlist::check(url)?;

    // Sufijo con guion, no con punto: el punto lo rechaza la validación de
    // nombres de archivo, y con razón. El archivo final es `<item_id>.mp4`, así
    // que el crudo necesita un nombre distinto que `cleanup` igual encuentre.
    let raw = ytdlp::download(
        &bins.ytdlp,
        url,
        platform,
        cfg.cookies.as_deref(),
        dest_dir,
        &format!("{item_id}-raw"),
        cfg.max_long_side,
        cfg.max_filesize_mb,
    )
    .await?;

    let info: VideoInfo = match ffmpeg::probe(&bins.ffprobe, &raw).await {
        Ok(i) => i,
        Err(e) => {
            let _ = std::fs::remove_file(&raw);
            return Err(e);
        }
    };

    let dims = ffmpeg::target_dims(info.width, info.height, cfg.max_short_side, cfg.max_long_side);
    let out_path = dest_dir.join(format!("{item_id}.mp4"));

    let encoder_used = match ffmpeg::transcode(
        &bins.ffmpeg,
        &raw,
        &out_path,
        dims,
        cfg.max_duration_seconds,
        cfg.encoder,
    )
    .await
    {
        Ok(enc) => enc,
        Err(e) => {
            let _ = std::fs::remove_file(&raw);
            let _ = std::fs::remove_file(&out_path);
            return Err(e);
        }
    };

    // El original se borra siempre: ya cumplió su función y es el único archivo
    // del proceso que no pasó por el decodificador de ffmpeg.
    let _ = std::fs::remove_file(&raw);

    // Se vuelve a medir sobre el archivo FINAL. Lo que importa para reproducir
    // es lo que quedó, no lo que dijo la metadata remota.
    let final_info = ffmpeg::probe(&bins.ffprobe, &out_path).await?;

    Ok(Prepared {
        item_id: item_id.to_string(),
        path: out_path,
        width: final_info.width,
        height: final_info.height,
        duration_seconds: final_info.duration_seconds,
        encoder_used,
    })
}

/// Borra los archivos de un pedido. Se usa al cancelar y al limpiar la cola
/// cuando termina el stream.
pub fn cleanup(dest_dir: &Path, item_id: &str) {
    let Ok(entries) = std::fs::read_dir(dest_dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if name.starts_with(item_id) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_borra_solo_lo_del_pedido() {
        let dir = std::env::temp_dir().join(format!("vrc-clean-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let id = "aaaabbbb";
        let _ = std::fs::write(dir.join(format!("{id}.mp4")), b"x");
        let _ = std::fs::write(dir.join(format!("{id}-raw.mp4")), b"x");
        let _ = std::fs::write(dir.join("otro-pedido.mp4"), b"x");

        cleanup(&dir, id);

        assert!(!dir.join(format!("{id}.mp4")).exists());
        assert!(!dir.join(format!("{id}-raw.mp4")).exists());
        assert!(dir.join("otro-pedido.mp4").exists(), "no debe tocar otros pedidos");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_config_por_defecto_respeta_el_plan() {
        let c = PipelineConfig::default();
        assert_eq!(c.max_duration_seconds, 30);
        assert_eq!(c.max_short_side, 720);
        assert_eq!(c.max_long_side, 1280);
        assert_eq!(c.encoder, Encoder::Nvenc);
        assert!(c.cookies.is_none());
    }
}
