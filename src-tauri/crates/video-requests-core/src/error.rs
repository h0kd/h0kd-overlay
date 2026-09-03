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

    /// Un fallo que por código no se reintenta pero que en la práctica es
    /// pasajero. TikTok, por ejemplo, contesta páginas vacías de a ratos: el
    /// extractor no está roto, es el sitio que no respondió esa vez.
    pub fn transient(code: ErrorCode, message: impl Into<String>) -> Self {
        ErrorDetail { code, message: message.into(), retryable: true }
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
    // Un post de Instagram que es foto o carrusel sin video. No es un fallo
    // nuestro ni del extractor: no hay nada que reproducir. El Worker ya no
    // deja mandar links /p/, pero un /reel/ que redirige a una foto llega acá.
    if s.contains("there is no video in this post") {
        return ErrorDetail::new(
            ErrorCode::NotFound,
            "Ese link es una foto o un post sin video. Solo se aceptan Reels.",
        );
    }
    // Un post de X sin video. "No suitable extractor" es el mismo caso con
    // un link adentro: el extractor de X quiso seguirlo y los extractores
    // acotados (ver ytdlp::base_args) no lo dejaron. Tampoco es un fallo.
    if s.contains("no video could be found in this tweet")
        || s.contains("no suitable extractor found")
    {
        return ErrorDetail::new(
            ErrorCode::NotFound,
            "Ese post de X no tiene video. Solo se aceptan posts con video.",
        );
    }
    // TikTok devuelve de a ratos una página sin los datos que el extractor
    // busca. Al siguiente intento suele andar: reintentable, no roto.
    if s.contains("universal data for rehydration")
        || s.contains("unexpected response from webpage request")
    {
        return ErrorDetail::transient(
            ErrorCode::ExtractorFailed,
            "TikTok no respondió bien esta vez; pasa seguido y se reintenta solo.",
        );
    }
    // Timeouts de red del propio yt-dlp (curl 28, read timed out…). No es un
    // extractor roto: es el sitio que tardó, y vale la pena volver a probar.
    if s.contains("timed out") || s.contains("curl: (28)") || s.contains("timeout") {
        return ErrorDetail::new(
            ErrorCode::Timeout,
            "El sitio tardó demasiado en responder.",
        );
    }
    if s.contains("connection reset")
        || s.contains("connection aborted")
        || s.contains("curl: (56)")
        || s.contains("curl: (35)")
        || s.contains("remote end closed connection")
    {
        return ErrorDetail::new(
            ErrorCode::DownloadFailed,
            "Se cortó la conexión con el sitio.",
        );
    }
    // Reel restringido por Instagram (edad o región): solo lo ve una cuenta
    // con sesión. Sin cookies no hay nada roto que actualizar; pasó en stream
    // y el mod leyó "probá actualizar yt-dlp" por un link que nunca iba a andar.
    if s.contains("isn't available to everyone")
        || s.contains("can't be seen by certain audiences")
    {
        return ErrorDetail::new(
            ErrorCode::NotFound,
            "Instagram restringe ese reel a ciertos públicos (edad o región): solo se ve \
             con una cuenta con sesión. Sin cookies de Instagram cargadas no se puede bajar.",
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
    fn foto_de_instagram_no_es_extractor_roto() {
        let out = "ERROR: [Instagram] DcBYXieBvua: There is no video in this post";
        let d = classify_ytdlp_stderr(out);
        assert_eq!(d.code, ErrorCode::NotFound);
        assert!(!d.retryable);
        assert!(d.message.contains("Reels"));
    }

    #[test]
    fn reel_restringido_no_es_extractor_roto() {
        let out = "ERROR: [Instagram] Dcgrv9ZIDmn: This content isn't available to everyone: It can't be seen by certain audiences.";
        let d = classify_ytdlp_stderr(out);
        assert_eq!(d.code, ErrorCode::NotFound);
        assert!(!d.retryable);
        assert!(d.message.contains("restringe"));
        assert!(!d.message.contains("yt-dlp"));
    }

    #[test]
    fn post_de_x_sin_video_no_es_extractor_roto() {
        for out in [
            "ERROR: [twitter] 1834334523344568597: No video could be found in this tweet",
            "ERROR: No suitable extractor found for URL https://go.nasa.gov/4aGEGua",
        ] {
            let d = classify_ytdlp_stderr(out);
            assert_eq!(d.code, ErrorCode::NotFound, "{out}");
            assert!(!d.retryable);
            assert!(d.message.contains("X"));
        }
    }

    #[test]
    fn tiktok_sin_datos_es_pasajero() {
        for out in [
            "ERROR: [TikTok] 764: Unable to extract universal data for rehydration; please report this issue",
            "ERROR: [TikTok] 764: Unexpected response from webpage request; please report this issue",
        ] {
            let d = classify_ytdlp_stderr(out);
            assert_eq!(d.code, ErrorCode::ExtractorFailed, "{out}");
            assert!(d.retryable, "tiene que reintentarse: {out}");
            assert!(!d.message.contains("actualizar yt-dlp"));
        }
    }

    #[test]
    fn timeout_de_red_es_reintentable() {
        let out = "ERROR: [Instagram] DcT: Unable to download webpage: Failed to perform, curl: (28) Connection timed out after 20009 milliseconds.";
        let d = classify_ytdlp_stderr(out);
        assert_eq!(d.code, ErrorCode::Timeout);
        assert!(d.retryable);
    }

    #[test]
    fn el_detalle_se_acota() {
        let largo = format!("ERROR: {}", "x".repeat(500));
        let d = classify_ytdlp_stderr(&largo);
        assert!(d.message.chars().count() < 350, "mensaje sin acotar: {}", d.message.len());
    }
}
