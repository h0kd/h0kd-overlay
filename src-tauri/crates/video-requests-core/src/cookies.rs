//! Cookies de Instagram.
//!
//! Instagram exige sesión logueada para que yt-dlp extraiga de forma confiable.
//! El agente guarda un archivo de cookies en formato Netscape, exportado de un
//! navegador logueado con una **cuenta dedicada** (nunca la personal del
//! streamer: si la banean, se crea otra y no se pierde nada).
//!
//! El archivo vive solo en la máquina del streamer. No va a la nube, no va al
//! repo, y no se manda por el WebSocket: al DO solo viaja el ESTADO.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookieState {
    Ok,
    Expired,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieStatus {
    pub present: bool,
    pub state: CookieState,
    pub last_ok_at: Option<u64>,
    pub last_error_at: Option<u64>,
}

impl Default for CookieStatus {
    fn default() -> Self {
        CookieStatus {
            present: false,
            state: CookieState::Missing,
            last_ok_at: None,
            last_error_at: None,
        }
    }
}

/// Dónde vive el archivo dentro del directorio de datos del agente.
pub fn cookies_path(data_dir: &Path) -> PathBuf {
    data_dir.join("video-requests").join("instagram-cookies.txt")
}

/// Estado inicial a partir del disco.
///
/// Que el archivo exista NO significa que sirva: eso solo lo dice usarlo. Por
/// eso arranca en `Ok` optimista y se degrada a `Expired` recién cuando una
/// extracción falla por login. Mentir para el otro lado (asumir vencidas)
/// haría que el agente no intente nunca.
pub fn read_status(data_dir: &Path) -> CookieStatus {
    let path = cookies_path(data_dir);
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > 0 => CookieStatus {
            present: true,
            state: CookieState::Ok,
            last_ok_at: None,
            last_error_at: None,
        },
        // Un archivo vacío es igual de inútil que ninguno, y es un caso real:
        // pasa cuando la exportación del navegador falla a mitad de camino.
        _ => CookieStatus::default(),
    }
}

/// Chequeo de forma del archivo Netscape, sin leerlo entero.
///
/// No valida que las cookies sirvan (eso no se puede saber sin usarlas), pero
/// sí atrapa el error más común: guardar un JSON exportado por una extensión
/// en vez del formato Netscape que espera yt-dlp.
pub fn looks_like_netscape(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let head: String = content.chars().take(4096).collect();
    let trimmed = head.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return false; // es JSON, no Netscape
    }
    head.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .any(|l| l.split('\t').count() >= 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_json_no_pasa_por_netscape() {
        let dir = std::env::temp_dir().join(format!("vrc-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("c.txt");
        let _ = std::fs::write(&p, r#"[{"name":"sessionid","value":"x"}]"#);
        assert!(!looks_like_netscape(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_netscape_de_verdad_pasa() {
        let dir = std::env::temp_dir().join(format!("vrc-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("c.txt");
        let _ = std::fs::write(
            &p,
            "# Netscape HTTP Cookie File\n.instagram.com\tTRUE\t/\tTRUE\t1800000000\tsessionid\tabc123\n",
        );
        assert!(looks_like_netscape(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_archivo_vacio_cuenta_como_ausente() {
        let dir = std::env::temp_dir().join(format!("vrc-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(dir.join("video-requests"));
        let _ = std::fs::write(cookies_path(&dir), "");
        let st = read_status(&dir);
        assert_eq!(st.state, CookieState::Missing);
        assert!(!st.present);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
