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
use crate::kappa;
use crate::ytdlp::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Cuántas veces en total se intenta algo que falla por un motivo pasajero.
const ATTEMPTS: u32 = 3;
/// Pausa antes de cada reintento. TikTok contesta vacío de a ratos e Instagram
/// corta la conexión de a ratos; en los dos casos unos segundos alcanzan.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(5)];

/// Reintenta `op` mientras el error diga `retryable`, hasta `ATTEMPTS` veces.
///
/// Existe porque en el primer stream real la mitad de los fallos fueron de
/// esta clase: un TikTok bueno que "no se pudo leer" y al toque sí, un
/// Instagram que cortó la conexión a mitad de descarga. Un pedido que falla
/// queda como fallido para el viewer y para el mod; vale más esperar cinco
/// segundos que hacerlos mandar el link de nuevo.
async fn with_retries<T, F, Fut>(op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    retry_with(op, &RETRY_DELAYS).await
}

async fn retry_with<T, F, Fut>(mut op: F, delays: &[Duration]) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0u32;
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) if e.retryable && attempt + 1 < ATTEMPTS => {
                let delay = delays
                    .get(attempt as usize)
                    .or(delays.last())
                    .copied()
                    .unwrap_or(Duration::ZERO);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(mut e) => {
                if attempt > 0 {
                    e.message = format!("{} (tras {} intentos)", e.message, attempt + 1);
                }
                return Err(e);
            }
        }
    }
}

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
    // kappa.lol sirve el archivo directo: yt-dlp no tiene de dónde sacar
    // título ni duración, así que la metadata se arma por otro camino.
    if platform == Platform::Kappa {
        let meta = kappa::fetch_metadata(&bins.ffprobe, url, cfg.max_filesize_mb).await?;
        return Ok((platform, meta));
    }
    let cookies = cfg.cookies.as_deref();
    let meta =
        with_retries(|| ytdlp::fetch_metadata(&bins.ytdlp, url, platform, cookies)).await?;
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
    let stem = format!("{item_id}-raw");
    let raw = with_retries(|| {
        ytdlp::download(
            &bins.ytdlp,
            url,
            platform,
            cfg.cookies.as_deref(),
            dest_dir,
            &stem,
            cfg.max_long_side,
            cfg.max_filesize_mb,
        )
    })
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
    use crate::error::{ErrorCode, ErrorDetail};
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn reintenta_solo_lo_pasajero_y_hasta_el_tope() {
        let veces = AtomicU32::new(0);
        let r: Result<()> = retry_with(
            || {
                veces.fetch_add(1, Ordering::SeqCst);
                async { Err(ErrorDetail::new(ErrorCode::Timeout, "tardó")) }
            },
            &[],
        )
        .await;
        assert_eq!(veces.load(Ordering::SeqCst), ATTEMPTS);
        let e = r.unwrap_err();
        assert!(e.message.contains("tras 3 intentos"), "{}", e.message);

        // Cookies vencidas no se reintentan: mil intentos dan mil fallos.
        let veces = AtomicU32::new(0);
        let r: Result<()> = retry_with(
            || {
                veces.fetch_add(1, Ordering::SeqCst);
                async { Err(ErrorDetail::new(ErrorCode::CookiesExpired, "vencidas")) }
            },
            &[],
        )
        .await;
        assert_eq!(veces.load(Ordering::SeqCst), 1);
        assert_eq!(r.unwrap_err().message, "vencidas");

        // Si el segundo intento anda, se devuelve eso y listo.
        let veces = AtomicU32::new(0);
        let r = retry_with(
            || {
                let n = veces.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n == 0 {
                        Err(ErrorDetail::transient(ErrorCode::ExtractorFailed, "vacío"))
                    } else {
                        Ok(n)
                    }
                }
            },
            &[],
        )
        .await;
        assert_eq!(r.ok(), Some(1));
    }

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
