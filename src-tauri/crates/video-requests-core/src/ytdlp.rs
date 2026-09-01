//! yt-dlp: metadata sin descargar, y descarga.
//!
//! Instagram es la plataforma prioritaria y la más hostil. Todo lo que hay acá
//! está pensado para esa realidad: cookies de una cuenta dedicada, ritmo bajo,
//! y fallos que terminan en un estado visible en vez de un cuelgue.

use crate::allowlist::Platform;
use crate::error::{classify_ytdlp_stderr, ErrorCode, ErrorDetail, Result};
use crate::proc;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

const METADATA_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Pausa entre requests que hace el propio yt-dlp. El volumen natural ya es
/// bajo (metadata al enviar, descarga al aprobar, uno a la vez), pero pedirle
/// ritmo explícito a Instagram es barato comparado con que banee la cuenta.
const SLEEP_REQUESTS: &str = "1.5";

#[derive(Debug, Clone)]
pub struct Metadata {
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail_url: Option<String>,
}

#[derive(Deserialize)]
struct DumpJson {
    title: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    #[serde(rename = "_type")]
    kind: Option<String>,
}

/// Argumentos comunes a metadata y descarga.
///
/// Las cookies solo se pasan en Instagram: TikTok, Twitch y YouTube funcionan
/// anónimos, y mandar la sesión de la cuenta dedicada a sitios que no la
/// necesitan es exposición gratis.
///
/// El ritmo bajo, en cambio, también va para TikTok: no pide login, pero corta
/// por ritmo igual que Instagram si le llueven pedidos seguidos, y en un stream
/// con cola le llueven.
fn base_args(platform: Platform, cookies: Option<&Path>) -> Vec<String> {
    let mut a = vec![
        "--no-playlist".to_string(),
        "--no-warnings".to_string(),
        // Sin colores ANSI: el stderr se parsea y se muestra en una UI.
        "--no-progress".to_string(),
    ];
    if platform == Platform::Instagram || platform == Platform::Tiktok {
        a.push("--sleep-requests".to_string());
        a.push(SLEEP_REQUESTS.to_string());
    }
    if platform == Platform::Instagram {
        if let Some(c) = cookies {
            a.push("--cookies".to_string());
            a.push(c.to_string_lossy().into_owned());
        }
    }
    a
}

/// Metadata sin bajar un solo byte de video.
///
/// Corre en el momento del envío, antes de que ningún mod vea nada: es lo que
/// permite rechazar por duración sin gastar ancho de banda ni disco, y lo que
/// le da al mod título y miniatura para decidir.
pub async fn fetch_metadata(
    ytdlp: &Path,
    url: &str,
    platform: Platform,
    cookies: Option<&Path>,
) -> Result<Metadata> {
    let mut args = base_args(platform, cookies);
    args.push("--dump-json".to_string());
    args.push("--simulate".to_string());
    args.push(url.to_string());

    let out = proc::run(ytdlp, &args, METADATA_TIMEOUT, "yt-dlp (metadata)").await?;
    if !out.status_ok {
        return Err(classify_ytdlp_stderr(&out.stderr));
    }

    // Con --no-playlist igual puede venir más de una línea; se toma la primera.
    let line = out.stdout.lines().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| {
        ErrorDetail::new(ErrorCode::ExtractorFailed, "yt-dlp no devolvió metadata.")
    })?;

    let j: DumpJson = serde_json::from_str(line).map_err(|e| {
        ErrorDetail::new(ErrorCode::ExtractorFailed, format!("Metadata ilegible: {e}"))
    })?;

    if j.kind.as_deref() == Some("playlist") {
        return Err(ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "Ese link es una lista, no un video suelto.",
        ));
    }

    Ok(Metadata {
        title: j.title,
        uploader: j.uploader.or(j.channel),
        duration_seconds: j.duration,
        thumbnail_url: j.thumbnail,
    })
}

/// Descarga el video a `dest_dir` con un nombre que elegimos nosotros.
///
/// El nombre sale del `item_id` (un UUID), **nunca** del título remoto: un
/// título controlado por un tercero es una vía directa a path traversal y a
/// nombres que el sistema de archivos no acepta.
pub async fn download(
    ytdlp: &Path,
    url: &str,
    platform: Platform,
    cookies: Option<&Path>,
    dest_dir: &Path,
    item_id: &str,
    max_height: u32,
    max_filesize_mb: u32,
) -> Result<PathBuf> {
    if !is_safe_stem(item_id) {
        return Err(ErrorDetail::new(
            ErrorCode::DownloadFailed,
            "El identificador del pedido tiene caracteres inválidos.",
        ));
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| {
        ErrorDetail::new(ErrorCode::DiskFull, format!("No se pudo crear la carpeta: {e}"))
    })?;

    let template = dest_dir.join(format!("{item_id}.%(ext)s"));
    let mut args = base_args(platform, cookies);
    args.extend([
        // Se pide directamente algo del tamaño que vamos a usar; bajar 4K para
        // reescalarlo a 720 es tiempo y ancho de banda tirados.
        "-f".to_string(),
        format!("bv*[height<={max_height}]+ba/b[height<={max_height}]/bv*+ba/b"),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "--max-filesize".to_string(),
        format!("{max_filesize_mb}M"),
        "--no-part".to_string(),
        "-o".to_string(),
        template.to_string_lossy().into_owned(),
        url.to_string(),
    ]);

    let out = proc::run(ytdlp, &args, DOWNLOAD_TIMEOUT, "yt-dlp (descarga)").await?;
    if !out.status_ok {
        return Err(classify_ytdlp_stderr(&out.stderr));
    }

    // `--max-filesize` hace que yt-dlp salga con éxito sin escribir nada, así
    // que "terminó bien" no alcanza: hay que ver si el archivo existe.
    let found = find_downloaded(dest_dir, item_id).ok_or_else(|| {
        if out.stdout.contains("larger than max-filesize")
            || out.stderr.contains("larger than max-filesize")
        {
            ErrorDetail::new(
                ErrorCode::TooLarge,
                format!("El video pesa más de {max_filesize_mb} MB."),
            )
        } else {
            ErrorDetail::new(
                ErrorCode::DownloadFailed,
                "yt-dlp terminó sin errores pero no dejó ningún archivo.",
            )
        }
    })?;

    Ok(found)
}

/// Busca el archivo que dejó yt-dlp: sabemos el nombre, no la extensión.
fn find_downloaded(dir: &Path, stem: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(stem))
}

/// Solo se aceptan identificadores que no puedan escaparse de la carpeta.
fn is_safe_stem(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Versión instalada, para reportarla y para saber si conviene actualizar.
pub async fn version(ytdlp: &Path) -> Option<String> {
    let out = proc::run(ytdlp, &["--version"], Duration::from_secs(20), "yt-dlp").await.ok()?;
    if !out.status_ok {
        return None;
    }
    let v = out.stdout.trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// `yt-dlp -U`. El extractor de Instagram se rompe y se arregla seguido, así
/// que mantenerlo al día es parte de que la feature siga funcionando.
pub async fn self_update(ytdlp: &Path) -> Result<String> {
    let out = proc::run(ytdlp, &["-U"], Duration::from_secs(180), "yt-dlp -U").await?;
    if !out.status_ok {
        return Err(ErrorDetail::new(
            ErrorCode::DownloadFailed,
            format!("No se pudo actualizar yt-dlp: {}", out.stderr.trim()),
        ));
    }
    Ok(out.stdout.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instagram_lleva_cookies_y_ritmo_bajo() {
        let c = PathBuf::from("cookies.txt");
        let args = base_args(Platform::Instagram, Some(&c));
        assert!(args.contains(&"--cookies".to_string()));
        assert!(args.contains(&"--sleep-requests".to_string()));
    }

    #[test]
    fn twitch_y_youtube_no_reciben_las_cookies() {
        // Mandar la sesión de la cuenta dedicada a sitios que no la piden es
        // exposición gratis.
        let c = PathBuf::from("cookies.txt");
        for p in [Platform::Twitch, Platform::Youtube] {
            let args = base_args(p, Some(&c));
            assert!(!args.contains(&"--cookies".to_string()), "{p} no debería llevar cookies");
            assert!(!args.iter().any(|a| a.contains("cookies.txt")));
        }
    }

    #[test]
    fn los_identificadores_peligrosos_se_rechazan() {
        assert!(is_safe_stem("66e04425-1d09-4315-9331-76f58b4ed3f6"));
        assert!(!is_safe_stem("../../../etc/passwd"));
        assert!(!is_safe_stem("con espacios"));
        assert!(!is_safe_stem("nombre.con.puntos"));
        assert!(!is_safe_stem(""));
        assert!(!is_safe_stem(&"x".repeat(65)));
    }
}
