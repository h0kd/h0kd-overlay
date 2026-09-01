//! Metadata de kappa.lol.
//!
//! kappa.lol no es una red social: es un host de archivos (el uploader que usa
//! Chatterino) que sirve el mp4 directo, sin página ni API de metadata. yt-dlp
//! lo baja bien con su extractor genérico, pero para la metadata devuelve el
//! id como título y ninguna duración, y la duración es justo lo que permite
//! rechazar un video largo antes de bajarlo.
//!
//! Acá se arma la metadata de otra forma: un HEAD para el nombre original del
//! archivo (viene en `Content-Disposition`) y el tamaño, y ffprobe sobre la
//! URL para la duración, que lee los encabezados del contenedor por rangos
//! HTTP sin bajar el archivo entero.

use crate::error::{ErrorCode, ErrorDetail, Result};
use crate::ffmpeg;
use crate::ytdlp::Metadata;
use std::path::Path;
use std::time::Duration;

const HEAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Metadata de un archivo de kappa.lol sin bajarlo.
///
/// `url` ya pasó por la allowlist. `max_filesize_mb` corta acá lo que yt-dlp
/// cortaría igual al bajar, pero con un mensaje claro y antes de que un mod
/// pierda tiempo mirándolo.
pub async fn fetch_metadata(ffprobe: &Path, url: &str, max_filesize_mb: u32) -> Result<Metadata> {
    let head = head(url).await?;

    if let Some(bytes) = head.content_length {
        if bytes > u64::from(max_filesize_mb) * 1024 * 1024 {
            return Err(ErrorDetail::new(
                ErrorCode::TooLarge,
                format!("El archivo pesa más de {max_filesize_mb} MB."),
            ));
        }
    }

    // ffprobe sobre la URL: si no es un video (una imagen, un zip con otro
    // nombre), acá se corta con el mismo mensaje que daría el pipeline local.
    let info = ffmpeg::probe_input(ffprobe, url).await?;

    Ok(Metadata {
        title: head.filename.map(|f| sin_extension(&f)),
        uploader: None,
        duration_seconds: (info.duration_seconds > 0.0).then_some(info.duration_seconds),
        thumbnail_url: None,
    })
}

struct Head {
    filename: Option<String>,
    content_length: Option<u64>,
}

async fn head(url: &str) -> Result<Head> {
    let client = reqwest::Client::builder()
        .timeout(HEAD_TIMEOUT)
        // Un redirect podría sacarnos del dominio que pasó la allowlist.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ErrorDetail::new(ErrorCode::ExtractorFailed, e.to_string()))?;

    let resp = client.head(url).send().await.map_err(|e| {
        ErrorDetail::new(ErrorCode::ExtractorFailed, format!("No se pudo consultar kappa.lol: {e}"))
    })?;

    match resp.status().as_u16() {
        200 => {}
        404 => {
            return Err(ErrorDetail::new(
                ErrorCode::NotFound,
                "Ese archivo ya no está en kappa.lol.",
            ))
        }
        429 => {
            return Err(ErrorDetail::new(
                ErrorCode::RateLimited,
                "kappa.lol está limitando las consultas; probá en un rato.",
            ))
        }
        s => {
            return Err(ErrorDetail::new(
                ErrorCode::ExtractorFailed,
                format!("kappa.lol respondió {s}."),
            ))
        }
    }

    let filename = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_filename);

    Ok(Head { filename, content_length: resp.content_length() })
}

/// Saca `filename="..."` (o `filename=...`) de un Content-Disposition. Solo se
/// usa como título para mostrar, nunca como nombre de archivo en disco.
fn parse_filename(header: &str) -> Option<String> {
    let idx = header.find("filename=")?;
    let rest = header[idx + "filename=".len()..].trim();
    let name = if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next().unwrap_or("")
    } else {
        rest.split(';').next().unwrap_or("").trim()
    };
    let name = name.trim();
    (!name.is_empty() && name.len() <= 200).then(|| name.to_string())
}

fn sin_extension(nombre: &str) -> String {
    match nombre.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() && ext.len() <= 5 => base.to_string(),
        _ => nombre.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_nombre_del_content_disposition() {
        assert_eq!(
            parse_filename(r#"inline; filename="AMEASUSTE.mp4""#).as_deref(),
            Some("AMEASUSTE.mp4")
        );
        assert_eq!(parse_filename("attachment; filename=clip.webm").as_deref(), Some("clip.webm"));
        assert_eq!(parse_filename("inline"), None);
        assert_eq!(parse_filename(r#"inline; filename="""#), None);
    }

    #[test]
    fn el_titulo_va_sin_extension() {
        assert_eq!(sin_extension("AMEASUSTE.mp4"), "AMEASUSTE");
        assert_eq!(sin_extension("sin punto"), "sin punto");
        assert_eq!(sin_extension("v1.2 final.mp4"), "v1.2 final");
        assert_eq!(sin_extension(".oculto"), ".oculto");
    }
}
