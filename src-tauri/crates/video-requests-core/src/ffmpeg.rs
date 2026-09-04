//! ffprobe y recodificación.
//!
//! La recodificación cumple tres funciones, y la del medio es la que importa
//! para la seguridad:
//!
//! 1. **Uniformidad**: todo termina en H.264 + AAC en MP4, que es lo único que
//!    el overlay tiene que saber reproducir.
//! 2. **Saneamiento**: ffmpeg decodifica a fotogramas crudos y reconstruye el
//!    archivo desde cero. Lo que no sea video y audio de verdad no sobrevive el
//!    viaje; el archivo descargado nunca se ejecuta, solo se decodifica.
//! 3. **Límites duros**: el corte de duración se aplica acá, no se confía en lo
//!    que dijo la metadata.

use crate::error::{ErrorCode, ErrorDetail, Result};
use crate::proc;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// Tope de tiempo para recodificar. Un clip de 30s con NVENC tarda segundos;
/// si pasa de esto, algo se colgó.
const TRANSCODE_TIMEOUT: Duration = Duration::from_secs(180);
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    /// NVENC: el streamer tiene una RTX, y esto no le roba CPU a OBS.
    Nvenc,
    /// Fallback por software para máquinas sin NVIDIA.
    X264,
}

impl Encoder {
    pub fn as_str(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::X264 => "libx264",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
}

#[derive(Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Confirma que el archivo tiene un stream de video real y devuelve sus datos.
///
/// Es el chequeo que separa "yt-dlp escribió algo" de "esto es un video". Si
/// falla, el ítem no llega a recodificarse y jamás llega al overlay.
pub async fn probe(ffprobe: &Path, input: &Path) -> Result<VideoInfo> {
    probe_input(ffprobe, &input.to_string_lossy()).await
}

/// Lo mismo que `probe`, pero sobre lo que ffprobe acepte como entrada: un
/// archivo o una URL https. Con una URL lee solo los encabezados del contenedor
/// (rangos HTTP), que es lo que permite saber la duración de un archivo directo
/// sin bajarlo. Quien llama es responsable de que la URL haya pasado la
/// allowlist: acá no se vuelve a validar.
pub async fn probe_input(ffprobe: &Path, input: &str) -> Result<VideoInfo> {
    let input_str = input.to_string();
    let args = vec![
        "-v".to_string(),
        "error".to_string(),
        "-print_format".to_string(),
        "json".to_string(),
        "-show_streams".to_string(),
        "-show_format".to_string(),
        input_str,
    ];

    let out = proc::run(ffprobe, &args, PROBE_TIMEOUT, "ffprobe").await?;
    if !out.status_ok {
        return Err(ErrorDetail::new(
            ErrorCode::ProbeFailed,
            format!("ffprobe no pudo leer el archivo: {}", out.stderr.trim()),
        ));
    }

    let parsed: ProbeOutput = serde_json::from_str(&out.stdout).map_err(|e| {
        ErrorDetail::new(ErrorCode::ProbeFailed, format!("Salida de ffprobe ilegible: {e}"))
    })?;

    let video = parsed
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| {
            ErrorDetail::new(
                ErrorCode::ProbeFailed,
                "El archivo no tiene ningún stream de video.",
            )
        })?;

    // Un "video" cuyo único stream es una imagen fija es un caso real: pasa
    // cuando el extractor devuelve la miniatura en vez del Reel.
    if matches!(video.codec_name.as_deref(), Some("mjpeg" | "png" | "gif" | "bmp")) {
        return Err(ErrorDetail::new(
            ErrorCode::ProbeFailed,
            "El archivo es una imagen, no un video.",
        ));
    }

    let (Some(width), Some(height)) = (video.width, video.height) else {
        return Err(ErrorDetail::new(
            ErrorCode::ProbeFailed,
            "No se pudieron leer las dimensiones del video.",
        ));
    };
    if width == 0 || height == 0 {
        return Err(ErrorDetail::new(ErrorCode::ProbeFailed, "El video mide 0 píxeles."));
    }

    let duration_seconds = video
        .duration
        .as_deref()
        .or(parsed.format.as_ref().and_then(|f| f.duration.as_deref()))
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);

    Ok(VideoInfo { width, height, duration_seconds })
}

/// Dimensiones de salida preservando la orientación.
///
/// Los Reels son verticales 9:16 y los clips de Twitch/YouTube horizontales.
/// Forzar 16:9 dejaría los Reels con barras negras enormes o recortados, así
/// que el tope se aplica por lado corto y lado largo, no por ancho y alto:
/// lado corto ≤ 720, lado largo ≤ 1280.
///
/// Nunca agranda: un video de 240p se queda en 240p, porque escalarlo hacia
/// arriba solo gasta bitrate para mostrar los mismos píxeles más grandes.
pub fn target_dims(width: u32, height: u32, max_short: u32, max_long: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (max_short, max_long);
    }
    let (short, long) = if width <= height { (width, height) } else { (height, width) };

    let scale_short = max_short as f64 / short as f64;
    let scale_long = max_long as f64 / long as f64;
    let scale = scale_short.min(scale_long).min(1.0); // .min(1.0) = nunca agrandar

    let w = even(((width as f64) * scale).round() as u32);
    let h = even(((height as f64) * scale).round() as u32);
    (w.max(2), h.max(2))
}

/// H.264 con submuestreo 4:2:0 necesita dimensiones pares o el encoder falla.
fn even(v: u32) -> u32 {
    v - (v % 2)
}

/// Argumentos de ffmpeg para la recodificación.
///
/// Separado de la ejecución para poder testear la línea exacta sin correr nada.
pub fn transcode_args(
    input: &Path,
    output: &Path,
    dims: (u32, u32),
    max_duration_seconds: u32,
    encoder: Encoder,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        // Corte duro de duración. Va DESPUÉS del -i para que aplique a la
        // salida: no se confía en lo que dijo la metadata.
        "-t".into(),
        max_duration_seconds.to_string(),
        "-vf".into(),
        format!("scale={}:{}", dims.0, dims.1),
        "-c:v".into(),
        encoder.as_str().into(),
    ];

    match encoder {
        Encoder::Nvenc => a.extend([
            "-preset".to_string(),
            "p5".to_string(),
            "-rc".to_string(),
            "vbr".to_string(),
            "-cq".to_string(),
            "28".to_string(),
            "-b:v".to_string(),
            "0".to_string(),
        ]),
        Encoder::X264 => a.extend([
            "-preset".to_string(),
            "veryfast".to_string(),
            "-crf".to_string(),
            "23".to_string(),
        ]),
    }

    a.extend([
        // yuv420p es lo único que reproducen todos los navegadores; sin esto,
        // una fuente en 4:4:4 sale como pantalla negra en el Browser Source.
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "128k".to_string(),
        "-ac".to_string(),
        "2".to_string(),
        // Manda el índice al principio: el <video> empieza a reproducir sin
        // tener que bajar el archivo entero primero.
        "-movflags".to_string(),
        "+faststart".to_string(),
        output.to_string_lossy().into_owned(),
    ]);
    a
}

/// Recodifica, con fallback a software si NVENC no está disponible.
///
/// El fallback existe porque NVENC falla de formas que no se pueden prever
/// desde acá: driver viejo, GPU sin encoder libre porque OBS lo tomó entero,
/// o una máquina sin NVIDIA. Fallar el pedido por eso sería peor que tardar
/// unos segundos más en CPU.
pub async fn transcode(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    dims: (u32, u32),
    max_duration_seconds: u32,
    encoder: Encoder,
) -> Result<Encoder> {
    let args = transcode_args(input, output, dims, max_duration_seconds, encoder);
    let out = proc::run(ffmpeg, &args, TRANSCODE_TIMEOUT, "ffmpeg").await?;
    if out.status_ok {
        return Ok(encoder);
    }

    if encoder == Encoder::Nvenc {
        let args = transcode_args(input, output, dims, max_duration_seconds, Encoder::X264);
        let out2 = proc::run(ffmpeg, &args, TRANSCODE_TIMEOUT, "ffmpeg (x264)").await?;
        if out2.status_ok {
            return Ok(Encoder::X264);
        }
        return Err(ErrorDetail::new(
            ErrorCode::TranscodeFailed,
            format!(
                "Falló la recodificación con NVENC y con x264. NVENC: {} | x264: {}",
                short(&out.stderr),
                short(&out2.stderr)
            ),
        ));
    }

    Err(ErrorDetail::new(
        ErrorCode::TranscodeFailed,
        format!("Falló la recodificación: {}", short(&out.stderr)),
    ))
}

fn short(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() > 200 {
        t.chars().take(200).collect::<String>() + "…"
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn un_reel_vertical_conserva_la_orientacion() {
        // 1080x1920 (9:16) -> lado corto a 720, largo a 1280.
        assert_eq!(target_dims(1080, 1920, 720, 1280), (720, 1280));
    }

    #[test]
    fn un_clip_horizontal_conserva_la_orientacion() {
        // 1920x1080 (16:9) -> 1280x720, no 720x1280.
        assert_eq!(target_dims(1920, 1080, 720, 1280), (1280, 720));
    }

    #[test]
    fn nunca_agranda() {
        assert_eq!(target_dims(480, 640, 720, 1280), (480, 640));
        assert_eq!(target_dims(320, 240, 720, 1280), (320, 240));
    }

    #[test]
    fn las_dimensiones_salen_pares() {
        // H.264 4:2:0 no acepta lados impares.
        for (w, h) in [(1079u32, 1921u32), (999, 333), (1443, 811), (777, 1001)] {
            let (tw, th) = target_dims(w, h, 720, 1280);
            assert_eq!(tw % 2, 0, "ancho impar para {w}x{h}: {tw}");
            assert_eq!(th % 2, 0, "alto impar para {w}x{h}: {th}");
        }
    }

    #[test]
    fn respeta_los_dos_topes_a_la_vez() {
        // Un panorámico extremo tiene que quedar limitado por el lado largo.
        let (w, h) = target_dims(3000, 500, 720, 1280);
        assert!(w <= 1280 && h <= 720, "{w}x{h} excede los topes");
        // Y mantener el aspecto original.
        let ratio_in = 3000.0 / 500.0;
        let ratio_out = w as f64 / h as f64;
        assert!((ratio_in - ratio_out).abs() < 0.15, "aspecto roto: {ratio_in} vs {ratio_out}");
    }

    #[test]
    fn un_cuadrado_se_limita_por_el_lado_corto() {
        assert_eq!(target_dims(1500, 1500, 720, 1280), (720, 720));
    }

    #[test]
    fn los_args_llevan_el_corte_de_duracion_despues_del_input() {
        let args = transcode_args(
            &PathBuf::from("in.mp4"),
            &PathBuf::from("out.mp4"),
            (720, 1280),
            30,
            Encoder::Nvenc,
        );
        let i = args.iter().position(|a| a == "-i").unwrap_or(usize::MAX);
        let t = args.iter().position(|a| a == "-t").unwrap_or(usize::MAX);
        assert!(i < t, "-t tiene que ir después de -i para cortar la salida");
        assert!(args.contains(&"30".to_string()));
    }

    #[test]
    fn los_args_fuerzan_yuv420p_y_faststart() {
        let args = transcode_args(
            &PathBuf::from("in.mp4"),
            &PathBuf::from("out.mp4"),
            (720, 1280),
            30,
            Encoder::X264,
        );
        assert!(args.contains(&"yuv420p".to_string()));
        assert!(args.contains(&"+faststart".to_string()));
        assert!(args.contains(&"libx264".to_string()));
    }

    #[test]
    fn el_escalado_va_como_filtro_explicito() {
        let args = transcode_args(
            &PathBuf::from("in.mp4"),
            &PathBuf::from("out.mp4"),
            (720, 1280),
            30,
            Encoder::Nvenc,
        );
        assert!(args.contains(&"scale=720:1280".to_string()));
    }
}
