//! `vrc-cli` — el pipeline de Video Requests desde la terminal.
//!
//! Existe para probar la parte frágil del sistema (la extracción de Instagram)
//! contra Reels reales, sin levantar la app, sin OBS y sin estar en vivo.
//!
//! ```text
//!   vrc-cli doctor
//!   vrc-cli install
//!   vrc-cli update
//!   vrc-cli meta   <url>
//!   vrc-cli fetch  <url> [--out DIR] [--id NOMBRE] [--x264] [--seconds N]
//! ```
//!
//! Las cookies de Instagram se toman de `<data>/video-requests/instagram-cookies.txt`,
//! donde `<data>` es `--data DIR` o, por defecto, la carpeta de la app
//! experimental.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use video_requests_core as core;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print_help();
        return ExitCode::SUCCESS;
    }

    let opts = Options::parse(&args);
    let data_dir = opts.data_dir.clone().unwrap_or_else(default_data_dir);

    let result = match args[0].as_str() {
        "doctor" => cmd_doctor(&data_dir).await,
        "install" => cmd_install(&data_dir).await,
        "update" => cmd_update(&data_dir).await,
        "meta" => match opts.positional.first() {
            Some(url) => cmd_meta(&data_dir, url, &opts).await,
            None => Err("falta la URL".to_string()),
        },
        "fetch" => match opts.positional.first() {
            Some(url) => cmd_fetch(&data_dir, url, &opts).await,
            None => Err("falta la URL".to_string()),
        },
        "transcode" => match opts.positional.first() {
            Some(file) => cmd_transcode(&data_dir, Path::new(file), &opts).await,
            None => Err("falta el archivo".to_string()),
        },
        other => Err(format!("comando desconocido: {other}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("\n  ERROR  {msg}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "vrc-cli — pipeline de Video Requests

  doctor                  Qué hay instalado, versiones y estado de las cookies
  install                 Baja yt-dlp y ffmpeg a la carpeta de datos
  update                  Actualiza yt-dlp (yt-dlp -U)
  meta  <url>             Metadata SIN descargar (título, duración, miniatura)
  fetch <url>             Descarga + valida + recodifica
  transcode <archivo>     Valida y recodifica un archivo LOCAL (sin red)

Opciones:
  --data DIR              Carpeta de datos (default: la de la app experimental)
  --out DIR               Dónde dejar el resultado de `fetch` (default: ./vrc-out)
  --id NOMBRE             Nombre base del archivo (default: un UUID)
  --cookies ARCHIVO       Cookies de Instagram (default: <data>/video-requests/instagram-cookies.txt)
  --seconds N             Duración máxima de la salida (default: 30)
  --x264                  Forzar encoder por software en vez de NVENC
"
    );
}

struct Options {
    data_dir: Option<PathBuf>,
    out: Option<PathBuf>,
    id: Option<String>,
    cookies: Option<PathBuf>,
    seconds: Option<u32>,
    x264: bool,
    positional: Vec<String>,
}

impl Options {
    fn parse(args: &[String]) -> Options {
        let mut o = Options {
            data_dir: None,
            out: None,
            id: None,
            cookies: None,
            seconds: None,
            x264: false,
            positional: Vec::new(),
        };
        let mut i = 1; // args[0] es el comando
        while i < args.len() {
            let a = args[i].as_str();
            let mut take = |o: &mut Option<PathBuf>| {
                if let Some(v) = args.get(i + 1) {
                    *o = Some(PathBuf::from(v));
                }
                i += 2;
            };
            match a {
                "--data" => take(&mut o.data_dir),
                "--out" => take(&mut o.out),
                "--cookies" => take(&mut o.cookies),
                "--id" => {
                    o.id = args.get(i + 1).cloned();
                    i += 2;
                }
                "--seconds" => {
                    o.seconds = args.get(i + 1).and_then(|v| v.parse().ok());
                    i += 2;
                }
                "--x264" => {
                    o.x264 = true;
                    i += 1;
                }
                _ => {
                    o.positional.push(args[i].clone());
                    i += 1;
                }
            }
        }
        o
    }
}

/// La misma carpeta que usa la app experimental, para compartir cookies y
/// binarios con ella.
fn default_data_dir() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("Stream Overlay Experimental")
}

fn config(data_dir: &Path, opts: &Options) -> core::PipelineConfig {
    let cookies = opts
        .cookies
        .clone()
        .or_else(|| {
            let p = core::cookies::cookies_path(data_dir);
            p.is_file().then_some(p)
        });
    core::PipelineConfig {
        max_duration_seconds: opts.seconds.unwrap_or(30),
        encoder: if opts.x264 { core::Encoder::X264 } else { core::Encoder::Nvenc },
        cookies,
        ..Default::default()
    }
}

fn si_no(v: bool) -> &'static str {
    if v {
        "sí"
    } else {
        "NO"
    }
}

async fn cmd_doctor(data_dir: &Path) -> std::result::Result<(), String> {
    let d = core::doctor(data_dir).await;
    println!("\n  Carpeta de datos   {}", data_dir.display());
    println!("  Carpeta de binarios {}", d.binaries.dir);
    println!("\n  yt-dlp    {}", d.ytdlp_version.clone().unwrap_or_else(|| "NO ENCONTRADO".into()));
    println!("  ffmpeg    {}", si_no(d.ffmpeg_found));
    println!("  ffprobe   {}", si_no(d.ffprobe_found));
    println!(
        "\n  cookies   {} ({:?}){}",
        si_no(d.cookies.present),
        d.cookies.state,
        if d.cookies.present && !d.cookies_look_valid {
            "  ← el archivo NO parece formato Netscape"
        } else {
            ""
        }
    );
    println!("  ruta      {}", core::cookies::cookies_path(data_dir).display());

    if d.ytdlp_version.is_none() || !d.ffmpeg_found || !d.ffprobe_found {
        println!("\n  Falta algo. Corré:  vrc-cli install");
    }
    if !d.cookies.present {
        println!(
            "\n  Sin cookies, Instagram va a fallar. Exportá las de la cuenta dedicada\n  \
             en formato Netscape y guardalas en la ruta de arriba."
        );
    }
    println!();
    Ok(())
}

async fn cmd_install(data_dir: &Path) -> std::result::Result<(), String> {
    println!("  Bajando lo que falte a {} …", core::binaries::bin_dir(data_dir).display());
    match core::binaries::install_missing(data_dir).await {
        Ok(done) if done.is_empty() => {
            println!("  Ya estaba todo instalado.");
            Ok(())
        }
        Ok(done) => {
            println!("  Listo: {}", done.join(", "));
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn cmd_update(data_dir: &Path) -> std::result::Result<(), String> {
    let bins = core::binaries::resolve(data_dir);
    match core::ytdlp::self_update(&bins.ytdlp).await {
        Ok(out) => {
            println!("{}", out.trim());
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn cmd_meta(
    data_dir: &Path,
    url: &str,
    opts: &Options,
) -> std::result::Result<(), String> {
    let bins = core::binaries::resolve(data_dir);
    let cfg = config(data_dir, opts);
    println!("  cookies: {}", cfg.cookies.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(ninguna)".into()));
    println!("  leyendo metadata…\n");

    let (platform, meta) = core::pipeline::fetch_metadata(&bins, url, &cfg)
        .await
        .map_err(|e| e.to_string())?;

    println!("  plataforma  {platform}");
    println!("  título      {}", meta.title.clone().unwrap_or_else(|| "(sin título)".into()));
    println!("  autor       {}", meta.uploader.clone().unwrap_or_else(|| "(desconocido)".into()));
    match meta.duration_seconds {
        Some(d) => {
            let over = d > cfg.max_duration_seconds as f64;
            println!(
                "  duración    {d:.1}s{}",
                if over { "   ← SUPERA EL MÁXIMO, se rechazaría solo" } else { "" }
            );
        }
        None => println!("  duración    (desconocida)"),
    }
    println!("  miniatura   {}", meta.thumbnail_url.clone().unwrap_or_else(|| "(ninguna)".into()));
    println!();
    Ok(())
}

/// Valida y recodifica un archivo que ya está en disco.
///
/// Sirve para probar la mitad de ffmpeg sin depender de que la extracción
/// funcione, y para diagnosticar un archivo que dio problemas.
async fn cmd_transcode(
    data_dir: &Path,
    input: &Path,
    opts: &Options,
) -> std::result::Result<(), String> {
    if !input.is_file() {
        return Err(format!("no existe el archivo: {}", input.display()));
    }
    let bins = core::binaries::resolve(data_dir);
    let cfg = config(data_dir, opts);
    let out_dir = opts.out.clone().unwrap_or_else(|| PathBuf::from("vrc-out"));
    let id = opts.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let info = core::ffmpeg::probe(&bins.ffprobe, input).await.map_err(|e| e.to_string())?;
    let dims = core::ffmpeg::target_dims(info.width, info.height, cfg.max_short_side, cfg.max_long_side);
    println!("  entrada    {}x{}  {:.1}s", info.width, info.height, info.duration_seconds);
    println!("  salida     {}x{}  (tope {}s)", dims.0, dims.1, cfg.max_duration_seconds);
    println!("
  recodificando…
");

    let out_path = out_dir.join(format!("{id}.mp4"));
    let started = std::time::Instant::now();
    let used = core::ffmpeg::transcode(
        &bins.ffmpeg,
        input,
        &out_path,
        dims,
        cfg.max_duration_seconds,
        cfg.encoder,
    )
    .await
    .map_err(|e| e.to_string())?;
    let elapsed = started.elapsed();

    let final_info = core::ffmpeg::probe(&bins.ffprobe, &out_path).await.map_err(|e| e.to_string())?;
    let size_mb = std::fs::metadata(&out_path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
    let orientation = if final_info.height > final_info.width { "vertical" } else { "horizontal" };

    println!("  LISTO en {:.1}s
", elapsed.as_secs_f64());
    println!("  archivo    {}", out_path.display());
    println!("  resultado  {}x{} ({orientation})  {:.1}s  {size_mb:.1} MB", final_info.width, final_info.height, final_info.duration_seconds);
    println!("  encoder    {}", used.as_str());
    if used == core::Encoder::X264 && cfg.encoder == core::Encoder::Nvenc {
        println!("
  Nota: NVENC falló y se usó x264 por software.");
    }
    println!();
    Ok(())
}

async fn cmd_fetch(
    data_dir: &Path,
    url: &str,
    opts: &Options,
) -> std::result::Result<(), String> {
    let bins = core::binaries::resolve(data_dir);
    let cfg = config(data_dir, opts);
    let out_dir = opts.out.clone().unwrap_or_else(|| PathBuf::from("vrc-out"));
    let id = opts.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    println!("  destino  {}", out_dir.display());
    println!("  id       {id}");
    println!("  encoder  {}", cfg.encoder.as_str());
    println!("  cookies  {}", cfg.cookies.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(ninguna)".into()));
    println!("\n  descargando, validando y recodificando…\n");

    let started = std::time::Instant::now();
    let prepared = core::pipeline::prepare(&bins, url, &id, &out_dir, &cfg)
        .await
        .map_err(|e| e.to_string())?;
    let elapsed = started.elapsed();

    let size_mb = std::fs::metadata(&prepared.path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
    let orientation = if prepared.height > prepared.width { "vertical" } else { "horizontal" };

    println!("  LISTO en {:.1}s\n", elapsed.as_secs_f64());
    println!("  archivo    {}", prepared.path.display());
    println!("  resolución {}x{} ({orientation})", prepared.width, prepared.height);
    println!("  duración   {:.1}s", prepared.duration_seconds);
    println!("  tamaño     {size_mb:.1} MB");
    println!("  encoder    {}", prepared.encoder_used.as_str());
    if prepared.encoder_used == core::Encoder::X264 && cfg.encoder == core::Encoder::Nvenc {
        println!("\n  Nota: NVENC falló y se usó x264 por software.");
    }
    println!();
    Ok(())
}
