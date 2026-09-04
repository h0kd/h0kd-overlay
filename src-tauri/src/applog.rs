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
//! Las horas son las del reloj del streamer, no UTC. Es lo que hace que el log
//! sirva: quien lo lee va a estar buscando "el video que no salió cuando eran
//! las once y media", y no tiene por qué convertir nada para encontrarlo.

use chrono::{Local, NaiveDate};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    let ahora = Local::now();
    let path = dir.join(format!("{}.log", ahora.format("%Y-%m-%d")));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", ahora.format("%H:%M:%S"));
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
    let hoy = Local::now().date_naive();
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(fecha) = name.strip_suffix(".log") else { continue };
        // Lo que no tiene forma de fecha no se toca: en esa carpeta puede haber
        // cualquier cosa, y borrar por las dudas es peor que dejar de más.
        let Ok(dia) = NaiveDate::parse_from_str(fecha, "%Y-%m-%d") else { continue };
        if (hoy - dia).num_days() > KEEP_DAYS {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_conservan_los_de_la_ultima_semana_y_se_borran_los_viejos() {
        let dir = std::env::temp_dir().join(format!("applog-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let hoy = Local::now().date_naive();
        let nombre = |d: i64| format!("{}.log", hoy - chrono::Duration::days(d));
        for d in [0, KEEP_DAYS, KEEP_DAYS + 1, 90] {
            fs::write(dir.join(nombre(d)), b"x").unwrap();
        }
        // Nombres que no son fechas: no son nuestros y no se borran.
        fs::write(dir.join("notas.log"), b"x").unwrap();
        fs::write(dir.join("cookies.txt"), b"x").unwrap();

        prune(&dir);

        assert!(dir.join(nombre(0)).exists(), "el de hoy tiene que quedar");
        assert!(dir.join(nombre(KEEP_DAYS)).exists(), "el del límite tiene que quedar");
        assert!(!dir.join(nombre(KEEP_DAYS + 1)).exists(), "el de más de una semana se va");
        assert!(!dir.join(nombre(90)).exists(), "el viejo se va");
        assert!(dir.join("notas.log").exists(), "lo que no es fecha no se toca");
        assert!(dir.join("cookies.txt").exists(), "lo que no es .log no se toca");

        let _ = fs::remove_dir_all(&dir);
    }
}
