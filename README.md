<div align="center">

<img src="src/assets/h0kdSPIN.webp" width="120" alt="Stream Overlay" />

# Stream Overlay

**Reproducí videos en tu stream cuando tus viewers canjean Channel Points en Twitch.**
App de escritorio self-hosted, sin servicios externos y sin límite de videos.

[![Descargar para Windows](https://img.shields.io/badge/⬇%20Descargar-Windows-9147ff?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/h0kd/h0kd-overlay/releases/latest)

<br/>

<img src="docs/screenshots/panel.png" width="760" alt="Panel de control de Stream Overlay" />

</div>

---

## ✨ Qué hace

- 🎬 **Dispara videos** en un overlay de OBS cuando canjean un reward de Channel Points.
- 🔀 **Varios videos por reward**: elige 1 al azar, o reproduce **todos a la vez**.
- 📐 **Tamaño, posición y volumen** por reward, respetando *safe zones* (que no tape tu webcam).
- ⏱️ **Duración automática**: detecta cuánto dura cada video para que no se corte.
- 🟣 **Conexión directa a Twitch** (EventSub) — **no necesitás Streamer.bot** ni nada más.
- 🔄 **Auto-actualizaciones**: te avisa cuando hay una versión nueva y se actualiza sola.
- 📨 **Video Requests** (beta, opcional): tus viewers mandan links de YouTube, TikTok, Instagram o X,
  tus mods los aprueban desde una web y salen en el overlay. [Cómo activarlo](docs/video-requests.md).

---

## ⬇️ Descargar e instalar

1. Bajá el instalador desde **[la última Release](https://github.com/h0kd/h0kd-overlay/releases/latest)** (`.exe` o `.msi`).
2. Ejecutalo.

   > **Nota:** la primera vez, Windows SmartScreen puede avisar que la app no es reconocida
   > (todavía no está firmada digitalmente). Hacé clic en **Más info → Ejecutar de todas formas**.

3. Abrí **Stream Overlay**. La primera vez te muestra una guía rápida de 3 pasos.

> Tu configuración y videos quedan en una carpeta tuya
> (`%APPDATA%\Stream Overlay`). El botón **Carpeta** de la app la abre.

---

## 🚀 Cómo usar

La primera vez, la app te recibe con una guía rápida:

<div align="center"><img src="docs/screenshots/onboarding.png" width="640" alt="Guía de bienvenida" /></div>

### 1. Conectá tu Twitch

En el panel **Twitch** (arriba a la izquierda) → **Conectar con Twitch**. La app te da un
código: andá a **[twitch.tv/activate](https://www.twitch.tv/activate)**, ingresalo y autorizá.
Listo — se reconecta solo la próxima vez.

### 2. Agregá tus videos y rewards

- Sumá tus videos (`.mp4` o `.webm`) con **+ Agregar**.
- Creá un reward con el **mismo nombre exacto** que tiene en Twitch (mayúsculas incluidas).
- Ajustá videos, modo de reproducción, volumen, tamaño y duración.

### 3. Conectá OBS

Agregá un **Browser Source** en OBS con esta URL:

```
http://localhost:3001/overlay
```

Width `1920` / Height `1080` (o tu resolución). Desmarcá *"Shutdown source when not visible"*
y *"Refresh browser when scene becomes active"*, y poné el source encima de todo.

> La app tiene que estar abierta para que el overlay funcione.

**Probá** un reward desde el panel (botón **▶ Probar**) sin gastar Channel Points.

---

## 📨 Video Requests (beta)

Un módulo opcional: los viewers pegan un link en `videos.h0kd.dev/submit?ch=<tu_canal>`,
los mods lo aprueban desde `videos.h0kd.dev/mod?ch=<tu_canal>` y la app lo baja y lo reproduce
en el mismo overlay, sin pisar los canjes. Viene apagado; apagado, la app funciona igual que siempre.

> **Alta manual, por ahora.** Cada canal se habilita a mano en el servidor. Pedilo con un
> [issue](https://github.com/h0kd/h0kd-overlay/issues/new) con tu nombre de canal, o escribile a h0kd.

En resumen, una vez habilitado:

1. Entrá a **[videos.h0kd.dev/admin](https://videos.h0kd.dev/admin)** con tu Twitch y marcá a tus mods.
2. En la app, pestaña **Video Requests**: **Activar módulo**, Guardar, reiniciar, **Instalar lo que falte**.
3. Generá un código en `/admin` (Ajustes) y tocá **Emparejar** en la app.
4. Compartí el link de viewers. Los envíos se abren solos cuando estás en vivo.

La guía completa, con la ventana de preview para VRChat, las cookies de Instagram y los problemas
comunes, está en **[docs/video-requests.md](docs/video-requests.md)**.

---

## ❓ Problemas comunes

| Problema | Solución |
|----------|----------|
| El canje no dispara el video | El nombre del reward debe ser **idéntico** al de Twitch (mayúsculas incluidas). |
| "Sin overlay conectado" al probar | El Browser Source debe apuntar a `http://localhost:3001/overlay` y la app estar abierta. |
| "Suscripción rechazada" | Autorizá con tu cuenta de **broadcaster** (la que tiene los Channel Points). |
| No se ve el video | Verificá que el archivo exista (el panel marca ⚠ si falta) y que sea `.mp4`/`.webm`. |
| Sin audio | Habilitá el audio del Browser Source en OBS y revisá el volumen del reward. |

---

## 🛠️ Para desarrolladores

<details>
<summary>Compilar desde el código</summary>

Multiplataforma (Windows y macOS). Requiere [Rust](https://rustup.rs) y el Tauri CLI:

```bash
cargo install tauri-cli --version "^2"
```

- **Windows:** Microsoft C++ Build Tools + WebView2 (viene con Windows 10/11).
- **macOS:** Xcode Command Line Tools (`xcode-select --install`).

```bash
cd src-tauri
cargo tauri dev      # desarrollo (o: cargo run)
cargo tauri build    # genera los instaladores del SO actual
```

**Estructura:**

```
src-tauri/src/lib.rs             ← comandos Tauri + arranque
src-tauri/src/server.rs          ← server axum (HTTP + WebSocket) en :3001
src-tauri/src/twitch.rs          ← OAuth Device Code Flow + cliente EventSub
src-tauri/src/video_requests.rs  ← agente de Video Requests (WebSocket con la nube, cola)
src-tauri/crates/video-requests-core/  ← descarga y transcodificación (yt-dlp, ffmpeg) + vrc-cli
src/control.html                 ← panel de control (UI nativa)
src/overlay.html                 ← overlay servido en /overlay
src/preview.html                 ← ventana de preview (VRChat)
cloud/                           ← Worker de Cloudflare + Durable Object + D1 (ver cloud/README.md)
docs/ws-protocol.md              ← contrato entre la app y la nube
docs/alta-de-canal.md            ← cómo habilitamos un canal (manual)
```

**Releases:** al pushear un tag `v*`, GitHub Actions compila el instalador de Windows,
lo firma para el auto-updater y crea un Release en borrador (ver `.github/workflows/release.yml`).

</details>

---

## 📄 Licencia

[MIT](LICENSE) — usalo, modificalo y compartilo libremente.
