//! Revalidación de dominios en el agente.
//!
//! El Worker ya filtró la URL antes de aceptarla. Esto lo hace de nuevo, y no
//! es paranoia decorativa: entre el Worker y acá está el Durable Object, la red
//! y un WebSocket. El agente es el que va a ejecutar yt-dlp sobre esa URL en la
//! máquina del streamer, así que no delega en nadie la decisión de qué dominios
//! toca. Si algún día el DO devolviera algo raro, se corta acá.

use crate::error::{ErrorCode, ErrorDetail, Result};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Instagram,
    Tiktok,
    Twitch,
    Youtube,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Instagram => "instagram",
            Platform::Tiktok => "tiktok",
            Platform::Twitch => "twitch",
            Platform::Youtube => "youtube",
        }
    }

    pub fn parse(s: &str) -> Option<Platform> {
        match s {
            "instagram" => Some(Platform::Instagram),
            "tiktok" => Some(Platform::Tiktok),
            "twitch" => Some(Platform::Twitch),
            "youtube" => Some(Platform::Youtube),
            _ => None,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

const ALLOWED: &[(&str, Platform)] = &[
    ("instagram.com", Platform::Instagram),
    // vm./vt.tiktok.com entran solos por la regla de sufijo: son los links
    // cortos que genera la propia app y redirigen dentro de TikTok.
    ("tiktok.com", Platform::Tiktok),
    ("twitch.tv", Platform::Twitch),
    ("clips.twitch.tv", Platform::Twitch),
    ("youtube.com", Platform::Youtube),
    ("youtu.be", Platform::Youtube),
];

/// Verifica que la URL siga siendo una que aceptamos tocar.
///
/// Se parsea a mano en vez de sumar una dependencia de URL: lo único que hace
/// falta es separar esquema, host y path, y rechazar todo lo que traiga
/// credenciales, puerto o forma inesperada.
pub fn check(url: &str) -> Result<Platform> {
    let url = url.trim();

    let rest = match url.strip_prefix("https://") {
        Some(r) => r,
        // http:// habilita redirects y hosts internos; ni se intenta.
        None => {
            return Err(ErrorDetail::new(
                ErrorCode::UnsupportedPlatform,
                "El link tiene que empezar con https://",
            ))
        }
    };

    let (authority, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, "/"),
    };

    if authority.contains('@') {
        return Err(ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "El link no puede llevar credenciales.",
        ));
    }
    if authority.contains(':') {
        return Err(ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "El link no puede especificar un puerto.",
        ));
    }

    let host = authority.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);

    let platform = ALLOWED
        .iter()
        .find(|(h, _)| host == *h || host.ends_with(&format!(".{h}")))
        .map(|(_, p)| *p)
        .ok_or_else(|| {
            ErrorDetail::new(
                ErrorCode::UnsupportedPlatform,
                format!("El dominio '{host}' no está en la allowlist."),
            )
        })?;

    if platform == Platform::Instagram && !is_instagram_media_path(path) {
        return Err(ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "De Instagram solo se aceptan Reels y posts de video.",
        ));
    }
    if platform == Platform::Tiktok && !is_tiktok_media_path(host, path) {
        return Err(ErrorDetail::new(
            ErrorCode::UnsupportedPlatform,
            "De TikTok solo se aceptan videos, no perfiles.",
        ));
    }

    Ok(platform)
}

/// TikTok tiene dos formas de link y hay que aceptar las dos, porque la app
/// comparte una y el navegador la otra:
///
/// - `tiktok.com/@usuario/video/123...` y `tiktok.com/t/CODIGO`, y
/// - `vm.tiktok.com/CODIGO` (el link corto), donde el path es un solo segmento
///   y no dice nada del contenido. Ese caso se acepta por el host: son dominios
///   de TikTok que solo redirigen adentro de TikTok.
///
/// Lo que se rechaza en todos los casos es el perfil pelado (`/@usuario`), que
/// para yt-dlp es una playlist entera y no un video.
fn is_tiktok_media_path(host: &str, path: &str) -> bool {
    let clean = path.split(['?', '#']).next().unwrap_or(path);
    let partes: Vec<&str> = clean.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();

    if host == "vm.tiktok.com" || host == "vt.tiktok.com" {
        return partes.len() == 1 && es_codigo(partes[0]);
    }
    match partes.as_slice() {
        [usuario, "video", id] => usuario.starts_with('@') && es_codigo(id),
        ["t", codigo] => es_codigo(codigo),
        _ => false,
    }
}

fn es_codigo(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_instagram_media_path(path: &str) -> bool {
    let clean = path.split(['?', '#']).next().unwrap_or(path);
    let mut parts = clean.trim_matches('/').split('/');
    let kind = parts.next().unwrap_or("");
    let id = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return false;
    }
    matches!(kind, "reel" | "reels" | "p" | "tv")
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_un_reel() {
        assert_eq!(check("https://instagram.com/reel/Dck2TlIO_yH/"), Ok(Platform::Instagram));
        assert_eq!(check("https://www.instagram.com/reel/Dck2TlIO_yH/"), Ok(Platform::Instagram));
    }

    #[test]
    fn acepta_twitch_y_youtube() {
        assert_eq!(check("https://clips.twitch.tv/AlgunClip"), Ok(Platform::Twitch));
        assert_eq!(check("https://youtu.be/abc123"), Ok(Platform::Youtube));
    }

    #[test]
    fn acepta_las_dos_formas_de_tiktok() {
        for u in [
            "https://www.tiktok.com/@alguien/video/7412345678901234567",
            "https://tiktok.com/t/ZM8abc123",
            "https://vm.tiktok.com/ZM8abc123",
            "https://vt.tiktok.com/ZM8abc123/",
        ] {
            assert_eq!(check(u).unwrap(), Platform::Tiktok, "{u}");
        }
    }

    #[test]
    fn de_tiktok_no_pasa_un_perfil() {
        for u in [
            "https://www.tiktok.com/@alguien",
            "https://www.tiktok.com/@alguien/video/",
            "https://www.tiktok.com/foo/bar/baz",
        ] {
            assert!(check(u).is_err(), "{u} no debería pasar");
        }
    }

    #[test]
    fn rechaza_dominios_parecidos() {
        // El sufijo tiene que ser un componente entero del host, no una subcadena.
        assert!(check("https://instagram.com.evil.com/reel/x/").is_err());
        assert!(check("https://notinstagram.com/reel/x/").is_err());
        assert!(check("https://evil.com/reel/x/").is_err());
    }

    #[test]
    fn rechaza_formas_peligrosas() {
        assert!(check("http://instagram.com/reel/x/").is_err());
        assert!(check("https://user:pass@instagram.com/reel/x/").is_err());
        assert!(check("https://instagram.com:8080/reel/x/").is_err());
        assert!(check("no es una url").is_err());
        assert!(check("").is_err());
    }

    #[test]
    fn de_instagram_solo_media() {
        assert!(check("https://instagram.com/algun_usuario").is_err());
        assert!(check("https://instagram.com/stories/user/123/").is_err());
        assert!(check("https://instagram.com/p/AbC123/").is_ok());
    }

    #[test]
    fn un_subdominio_permitido_pasa() {
        assert_eq!(check("https://m.youtube.com/watch"), Ok(Platform::Youtube));
    }
}

// `assert_eq!` sobre Result necesita PartialEq en el error.
impl PartialEq for ErrorDetail {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.message == other.message
    }
}
