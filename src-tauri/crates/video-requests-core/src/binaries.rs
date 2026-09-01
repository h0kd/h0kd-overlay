//! Resolución e instalación de yt-dlp y ffmpeg.
//!
//! Se bajan al directorio de datos del agente la primera vez que se enciende el
//! módulo, en vez de meterlos en el instalador. Dos motivos: el instalador de la
//! app no engorda ~100 MB para todos los que no usan esta feature, y sobre todo
//! yt-dlp queda actualizable por su cuenta — el extractor de Instagram se rompe
//! y se arregla seguido, y esperar a un release de la app para arreglarlo
//! significaría tener la feature caída durante días.
//!
//! Si el usuario ya tiene los binarios en el PATH, se usan esos y no se baja
//! nada.

use crate::error::{ErrorCode, ErrorDetail, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

const YTDLP_URL_WIN: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const YTDLP_SUMS: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
const FFMPEG_ZIP: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
const FFMPEG_SHA: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256";

#[derive(Debug, Clone)]
pub struct Binaries {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BinaryStatus {
    pub ytdlp_present: bool,
    pub ffmpeg_present: bool,
    pub ffprobe_present: bool,
    pub dir: String,
}

/// Carpeta propia dentro del directorio de datos del agente.
pub fn bin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("video-requests").join("bin")
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Elige qué binarios usar: los instalados por la app si existen, y si no el
/// nombre pelado, que deja que el sistema lo resuelva por PATH.
pub fn resolve(data_dir: &Path) -> Binaries {
    let dir = bin_dir(data_dir);
    let pick = |name: &str| -> PathBuf {
        let local = dir.join(exe(name));
        if local.is_file() {
            local
        } else {
            PathBuf::from(name)
        }
    };
    Binaries { ytdlp: pick("yt-dlp"), ffmpeg: pick("ffmpeg"), ffprobe: pick("ffprobe") }
}

/// Qué hay instalado localmente. No mira el PATH: sirve para decidir si hay que
/// bajar, no para decidir si se puede correr.
pub fn status(data_dir: &Path) -> BinaryStatus {
    let dir = bin_dir(data_dir);
    BinaryStatus {
        ytdlp_present: dir.join(exe("yt-dlp")).is_file(),
        ffmpeg_present: dir.join(exe("ffmpeg")).is_file(),
        ffprobe_present: dir.join(exe("ffprobe")).is_file(),
        dir: dir.display().to_string(),
    }
}

/// Baja lo que falte. Devuelve qué hizo, para poder mostrarlo.
pub async fn install_missing(data_dir: &Path) -> Result<Vec<String>> {
    if !cfg!(windows) {
        return Err(ErrorDetail::new(
            ErrorCode::BinaryMissing,
            "La descarga automática solo está implementada para Windows. \
             Instalá yt-dlp y ffmpeg con el gestor de paquetes de tu sistema.",
        ));
    }

    let dir = bin_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| {
        ErrorDetail::new(ErrorCode::DiskFull, format!("No se pudo crear {}: {e}", dir.display()))
    })?;

    let mut done = Vec::new();

    if !dir.join(exe("yt-dlp")).is_file() {
        install_ytdlp(&dir).await?;
        done.push("yt-dlp".to_string());
    }
    if !dir.join(exe("ffmpeg")).is_file() || !dir.join(exe("ffprobe")).is_file() {
        install_ffmpeg(&dir).await?;
        done.push("ffmpeg + ffprobe".to_string());
    }
    Ok(done)
}

async fn install_ytdlp(dir: &Path) -> Result<()> {
    let dest = dir.join(exe("yt-dlp"));
    let tmp = dir.join("yt-dlp.download");

    let digest = download_to(YTDLP_URL_WIN, &tmp).await?;

    // El checksum se baja del mismo release. Esto detecta descargas truncadas o
    // corrompidas por la red, que es el fallo real y frecuente; no protege
    // contra un upstream comprometido. Fijar el hash en el código sí lo haría,
    // pero congelaría la versión, y acá justamente hace falta lo contrario.
    match fetch_text(YTDLP_SUMS).await {
        Ok(sums) => {
            if let Some(expected) = find_sum(&sums, "yt-dlp.exe") {
                if !expected.eq_ignore_ascii_case(&digest) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(ErrorDetail::new(
                        ErrorCode::DownloadFailed,
                        "El checksum de yt-dlp no coincide; se descartó la descarga.",
                    ));
                }
            }
        }
        // Sin checksum se sigue: quedarse sin la feature porque no se pudo leer
        // un archivo de sumas es peor que el riesgo que cubre.
        Err(_) => {}
    }

    std::fs::rename(&tmp, &dest).map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("No se pudo guardar yt-dlp: {e}"))
    })?;
    Ok(())
}

async fn install_ffmpeg(dir: &Path) -> Result<()> {
    let tmp = dir.join("ffmpeg.zip");
    let digest = download_to(FFMPEG_ZIP, &tmp).await?;

    if let Ok(sha) = fetch_text(FFMPEG_SHA).await {
        let expected = sha.split_whitespace().next().unwrap_or("").to_string();
        if !expected.is_empty() && !expected.eq_ignore_ascii_case(&digest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(ErrorDetail::new(
                ErrorCode::DownloadFailed,
                "El checksum de ffmpeg no coincide; se descartó la descarga.",
            ));
        }
    }

    let file = std::fs::File::open(&tmp).map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("No se pudo abrir el zip: {e}"))
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("Zip de ffmpeg ilegible: {e}"))
    })?;

    let wanted = [exe("ffmpeg"), exe("ffprobe")];
    let mut extracted = 0;
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else { continue };
        if !entry.is_file() {
            continue;
        }
        // El nombre dentro del zip viene del archivo: se usa SOLO el último
        // componente, nunca la ruta completa, para que no pueda escribir fuera
        // de la carpeta (zip slip).
        let name = entry.name().replace('\\', "/");
        let Some(base) = name.rsplit('/').next() else { continue };
        if !wanted.iter().any(|w| w == base) {
            continue;
        }
        let dest = dir.join(base);
        let mut out = std::fs::File::create(&dest).map_err(|e| {
            ErrorDetail::new(
                ErrorCode::DiskFull,
                format!("No se pudo escribir {}: {e}", dest.display()),
            )
        })?;
        std::io::copy(&mut entry, &mut out).map_err(|e| {
            ErrorDetail::new(ErrorCode::DiskFull, format!("Falló la extracción: {e}"))
        })?;
        extracted += 1;
    }

    let _ = std::fs::remove_file(&tmp);
    if extracted < 2 {
        return Err(ErrorDetail::new(
            ErrorCode::DownloadFailed,
            "El zip de ffmpeg no contenía ffmpeg y ffprobe.",
        ));
    }
    Ok(())
}

/// Descarga a un archivo y devuelve su SHA-256 en hex.
///
/// Se escribe a disco a medida que llega en vez de juntar todo en memoria: el
/// zip de ffmpeg pesa ~110 MB y esto corre en la máquina de alguien que está
/// transmitiendo.
async fn download_to(url: &str, dest: &Path) -> Result<String> {
    let out = stream_to_file(url, dest).await;
    if out.is_err() {
        // Un archivo a medias son decenas de MB de basura en la carpeta del
        // usuario. El próximo intento lo pisa, pero si abandona queda ahí.
        let _ = std::fs::remove_file(dest);
    }
    out
}

async fn stream_to_file(url: &str, dest: &Path) -> Result<String> {
    // Los timeouts son por INACTIVIDAD, nunca por duración total. Un tope total
    // mata descargas sanas: ffmpeg pesa ~110 MB, y con los 600 s que había acá
    // antes, cualquiera que bajara a menos de ~190 KB/s no podía instalarlo
    // nunca, por más que la descarga estuviera avanzando bien. Lo que hay que
    // cortar es la conexión que se quedó muda, no la que va lenta.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ErrorDetail::new(ErrorCode::DownloadFailed, format!("Cliente HTTP: {e}")))?;

    let resp = client.get(url).send().await.map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("No se pudo bajar {url}: {e}"))
    })?;
    if !resp.status().is_success() {
        return Err(ErrorDetail::new(
            ErrorCode::DownloadFailed,
            format!("{url} devolvió {}", resp.status()),
        ));
    }

    let mut file = std::fs::File::create(dest).map_err(|e| {
        ErrorDetail::new(ErrorCode::DiskFull, format!("No se pudo crear {}: {e}", dest.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            ErrorDetail::new(ErrorCode::DownloadFailed, format!("Se cortó la descarga: {e}"))
        })?;
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| {
            ErrorDetail::new(ErrorCode::DiskFull, format!("No se pudo escribir: {e}"))
        })?;
    }
    file.flush().ok();

    Ok(hex(&hasher.finalize()))
}

async fn fetch_text(url: &str) -> Result<String> {
    let resp = reqwest::get(url).await.map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("No se pudo bajar {url}: {e}"))
    })?;
    resp.text().await.map_err(|e| {
        ErrorDetail::new(ErrorCode::DownloadFailed, format!("Respuesta ilegible: {e}"))
    })
}

/// Busca `<hash>  <nombre>` en un archivo de sumas estilo sha256sum.
fn find_sum(sums: &str, filename: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        if name.trim_start_matches('*') == filename {
            Some(hash.to_string())
        } else {
            None
        }
    })
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` a un String no puede fallar, pero no se usa unwrap igual.
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_el_hash_por_nombre() {
        let sums = "aaaa  yt-dlp\nbbbb  yt-dlp.exe\ncccc  yt-dlp_macos\n";
        assert_eq!(find_sum(sums, "yt-dlp.exe").as_deref(), Some("bbbb"));
        assert_eq!(find_sum(sums, "yt-dlp").as_deref(), Some("aaaa"));
        assert_eq!(find_sum(sums, "no-esta"), None);
    }

    #[test]
    fn acepta_el_asterisco_del_modo_binario() {
        let sums = "dddd *yt-dlp.exe\n";
        assert_eq!(find_sum(sums, "yt-dlp.exe").as_deref(), Some("dddd"));
    }

    #[test]
    fn el_hex_sale_con_padding() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn sin_binarios_locales_cae_al_path() {
        let dir = std::env::temp_dir().join(format!("vrc-bin-{}", uuid::Uuid::new_v4()));
        let b = resolve(&dir);
        // Nombre pelado = que lo resuelva el sistema por PATH.
        assert_eq!(b.ytdlp, PathBuf::from("yt-dlp"));
        assert_eq!(b.ffprobe, PathBuf::from("ffprobe"));
    }
}
