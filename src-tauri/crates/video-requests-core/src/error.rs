//! Errores del pipeline.
//!
//! Los códigos son los mismos que define `docs/ws-protocol.md`: lo que sale de
//! acá viaja tal cual al Durable Object y termina mostrándose en el panel de
//! mods, así que el `message` está en español y escrito para un humano.
//!
//! Nada en este crate hace `panic!`, `unwrap()` ni `expect()`. El binario final
//! se compila con `panic = "abort"`, o sea que un panic acá se lleva puesta la
//! app entera a mitad de stream.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // Extracción / plataforma
    CookiesExpired,
    RateLimited,
    ExtractorFailed,
    UnsupportedPlatform,
    NotFound,
    // Política
    TooLong,
    TooLarge,
    Timeout,
    // Pipeline local
    DownloadFailed,
    ProbeFailed,
    TranscodeFailed,
    DiskFull,
    BinaryMissing,
    // Protocolo / control
    Cancelled,
}

impl ErrorCode {
    /// Si reintentar lo mismo tiene alguna chance de andar. `cookies_expired`
    /// no es reintentable: hasta que un humano no renueve el archivo, mil
    /// reintentos dan mil fallos y además apuran el bloqueo de la cuenta.
    pub fn retryable(self) -> bool {
        matches!(self, ErrorCode::RateLimited | ErrorCode::Timeout | ErrorCode::DownloadFailed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub code: ErrorCode,
    /// En español y mostrable tal cual al mod y en la UI del agente.
    pub message: String,
    pub retryable: bool,
}

impl ErrorDetail {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ErrorDetail { code, message: message.into(), retryable: code.retryable() }
    }
}

impl fmt::Display for ErrorDetail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for ErrorDetail {}

pub type Result<T> = std::result::Result<T, ErrorDetail>;

/// Clasifica el stderr de yt-dlp en un código accionable.
///
/// Importa distinguir estos casos porque llevan a acciones distintas: cookies
/// vencidas necesitan que un humano exporte el archivo de nuevo, un rate-limit
/// necesita esperar, y un extractor roto necesita actualizar yt-dlp. Reportar
/// todo como "falló la descarga" deja al operador sin saber qué hacer.
pub fn classify_ytdlp_stderr(stderr: &str) -> ErrorDetail {
    let s = stderr.to_ascii_lowercase();

    // El orden importa: login y rate-limit son más específicos que "falló".
    if s.contains("login required")
        || s.contains("requested content is not available")
        || s.contains("use --cookies")
        || s.contains("cookies-from-browser")
        || s.contains("rate-limit reached or login required")
        || s.contains("empty media response")
    {
        return ErrorDetail::new(
            ErrorCode::CookiesExpired,
            "Instagram pide sesión: las cookies están vencidas o no sirven. \
             Exportá de nuevo el archivo desde la cuenta dedicada.",
        );
    }
    if s.contains("http error 429") || s.contains("too many requests") || s.contains("rate limit") {
        return ErrorDetail::new(
            ErrorCode::RateLimited,
            "Instagram nos frenó por exceso de pedidos. Esperá un rato antes de reintentar.",
        );
    }
    if s.contains("video unavailable")
        || s.contains("http error 404")
        || s.contains("this post is unavailable")
        || s.contains("removed")
        || s.contains("private")
    {
        return ErrorDetail::new(
            ErrorCode::NotFound,
            "El video no existe, es privado o fue borrado.",
        );
    }
    if s.contains("file is larger than max-filesize") || s.contains("max-filesize") {
        return ErrorDetail::new(ErrorCode::TooLarge, "El archivo supera el tamaño máximo.");
    }
    if s.contains("unsupported url") || s.contains("no suitable extractor") {
        return ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "yt-dlp no tiene extractor para ese link.",
        );
    }

    // Todo lo demás en Instagram suele ser el extractor roto: se rompe y se
    // arregla seguido, y la salida es actualizar yt-dlp.
    ErrorDetail::new(
        ErrorCode::ExtractorFailed,
        format!(
            "yt-dlp no pudo leer el video. Puede que el extractor esté roto; \
             probá actualizar yt-dlp. Detalle: {}",
            first_useful_line(stderr)
        ),
    )
}

/// La primera línea de error de verdad del stderr, acotada.
///
/// yt-dlp escupe muchas líneas de warning antes del error real, y mandar el
/// stderr entero a la UI de un mod no ayuda a nadie.
fn first_useful_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("ERROR") || l.to_ascii_lowercase().contains("error"))
        .or_else(|| stderr.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("sin detalle");
    let line = line.trim();
    if line.chars().count() > 200 {
        let cut: String = line.chars().take(200).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_cookies_vencidas() {
        let out = "ERROR: [Instagram] DCk2: Requested content is not available, rate-limit reached or login required.";
        assert_eq!(classify_ytdlp_stderr(out).code, ErrorCode::CookiesExpired);
    }

    #[test]
    fn detecta_rate_limit() {
        let out = "ERROR: unable to download video data: HTTP Error 429: Too Many Requests";
        assert_eq!(classify_ytdlp_stderr(out).code, ErrorCode::RateLimited);
    }

    #[test]
    fn detecta_borrado_o_privado() {
        let out = "ERROR: [Instagram] abc: This post is unavailable.";
        assert_eq!(classify_ytdlp_stderr(out).code, ErrorCode::NotFound);
    }

    #[test]
    fn el_resto_cae_en_extractor_roto() {
        let out = "ERROR: [Instagram] xyz: Unable to extract shared data; please report this issue";
        let d = classify_ytdlp_stderr(out);
        assert_eq!(d.code, ErrorCode::ExtractorFailed);
        assert!(d.message.contains("actualizar yt-dlp"));
    }

    #[test]
    fn cookies_vencidas_no_es_reintentable() {
        // Reintentar con cookies muertas solo apura el bloqueo de la cuenta.
        assert!(!ErrorCode::CookiesExpired.retryable());
        assert!(ErrorCode::RateLimited.retryable());
    }

    #[test]
    fn el_detalle_se_acota() {
        let largo = format!("ERROR: {}", "x".repeat(500));
        let d = classify_ytdlp_stderr(&largo);
        assert!(d.message.chars().count() < 350, "mensaje sin acotar: {}", d.message.len());
    }
}
