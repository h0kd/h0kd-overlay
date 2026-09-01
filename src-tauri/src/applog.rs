//! Log a archivo.
//!
//! El build de release no tiene consola (`windows_subsystem = "windows"` en
//! `main.rs`), así que todo lo que se imprime durante un stream se lo lleva el
//! viento. Cuando el streamer dice "el video no se reprodujo" y la transmisión
//! terminó hace tres horas, esto es lo único que queda para saber qué pasó.
//!
//! Un archivo por día en `<data_dir>/logs/`, y se borran los viejos: un log que
//! crece sin límite en la máquina de otro es un problema, no una herramienta.
//!
//! **Las horas son UTC**, a propósito. Sacar la hora local sin sumar una
//! dependencia significa llamar a la API de Windows, y no vale una caja nueva
//! en el Cargo.toml por un formato de fecha. Lo que importa para diagnosticar
//! es el orden y la distancia entre los eventos, y eso UTC lo da igual.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Días de log que se conservan.
const KEEP_DAYS: i64 = 7;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Deja el log listo. Si algo de esto falla, `line()` no hace nada y la app
/// sigue andando: quedarse sin log es molesto, no arrancar es peor.
pub fn init(data_dir: &Path) {
    let dir = data_dir.join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    prune(&dir);
    let _ = DIR.set(dir);
}

/// Carpeta donde viven los logs, para poder abrirla desde la UI.
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

/// Escribe una línea. El archivo se abre y se cierra en cada llamada: son unos
/// pocos renglones por video, y así no hay que sostener un descriptor abierto
/// ni preocuparse por vaciar el buffer si la app se cierra de golpe.
pub fn line(msg: &str) {
    let Some(dir) = DIR.get() else { return };
    let (y, mo, d, h, mi, s) = now_utc();
    let path = dir.join(format!("{y:04}-{mo:02}-{d:02}.log"));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{h:02}:{mi:02}:{s:02}Z {msg}");
    }
}

/// Imprime por consola y además deja la línea en el archivo.
///
/// Se usa igual que `println!`. En debug se ve en la terminal; en release, que
/// no tiene consola, el archivo es la única copia.
#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => {{
        let s = format!($($arg)*);
        println!("{s}");
        $crate::applog::line(&s);
    }};
}

fn prune(dir: &Path) {
    let hoy = days_from_epoch();
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(fecha) = name.strip_suffix(".log") else { continue };
        let Some(dias) = days_from_name(fecha) else { continue };
        if hoy - dias > KEEP_DAYS {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// `YYYY-MM-DD` -> días desde epoch. `None` si el nombre no tiene esa forma:
/// en esa carpeta puede haber cualquier cosa y no se borra lo que no se entiende.
fn days_from_name(s: &str) -> Option<i64> {
    let mut p = s.split('-');
    let y: i64 = p.next()?.parse().ok()?;
    let m: u32 = p.next()?.parse().ok()?;
    let d: u32 = p.next()?.parse().ok()?;
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

fn days_from_epoch() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    secs.div_euclid(86_400)
}

fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let resto = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    (
        y,
        m,
        d,
        (resto / 3600) as u32,
        ((resto % 3600) / 60) as u32,
        (resto % 60) as u32,
    )
}

// Las dos conversiones de abajo son el algoritmo de calendario de Howard
// Hinnant (dominio público, http://howardhinnant.github.io/date_algorithms.html).
// Están acá para no sumar `chrono` al proyecto por un nombre de archivo.

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_dos_conversiones_son_inversas() {
        for (y, m, d) in [(1970, 1, 1), (2026, 8, 31), (2024, 2, 29), (2000, 12, 31)] {
            let dias = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(dias), (y, m, d), "{y}-{m}-{d}");
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn solo_se_entienden_los_nombres_con_forma_de_fecha() {
        assert_eq!(days_from_name("2026-08-31"), Some(days_from_civil(2026, 8, 31)));
        assert_eq!(days_from_name("cookies"), None);
        assert_eq!(days_from_name("2026-13-01"), None);
        assert_eq!(days_from_name("2026-08-31-viejo"), None);
    }
}
