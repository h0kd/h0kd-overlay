# Dar de alta un canal en Video Requests

Cómo sumar un streamer nuevo al mismo Worker. Cada canal es independiente:
cola, mods, historial y ajustes propios. Probado el 2026-09-01 con el segundo
canal (`iza42`).

## 1. Del lado del Worker (lo hace quien administra el Worker)

1. En `cloud/wrangler.toml`, agregar el login de Twitch a la lista del beta:
   ```toml
   ALLOWED_CHANNELS = "h0kd,iza42"
   ```
   Sin ese login, al entrar a `/admin` la página dice que el canal "todavía
   no está habilitado en el beta".
2. `cd cloud && npx wrangler deploy`.

Con más de un canal, `/submit` y `/mod` sin `?ch=` ya no redirigen solos:
cada canal comparte sus links con `?ch=<login>` (están en su `/admin`).

## 2. Del lado del streamer (en la web)

1. Entrar a `https://videos.h0kd.dev/admin` con su cuenta de Twitch. Eso da
   de alta el canal, guarda su token de Helix y crea las suscripciones de
   EventSub (los envíos se abren y cierran solos con su stream). Queda en
   `/admin?ch=<login>`.
2. Pestaña **Moderadores**: marcar a quién le da acceso al panel `/mod`.
   Ser mod en Twitch no alcanza; hay que marcarlo acá.
3. Pestaña **Ajustes**: ahí están los links para compartir
   (`/submit?ch=<login>` para viewers, `/mod?ch=<login>` para mods) y el
   botón para generar el código de emparejamiento.

## 3. Del lado del streamer (la app de escritorio)

1. Instalar la app desde la release de GitHub (`exp-v*`, canal experimental).
   Se instala al lado de la estable, con carpeta de datos y puerto propios.
2. Abrirla una vez y cerrarla: crea `%APPDATA%\Stream Overlay Experimental\config.json`.
3. Editar ese archivo. Viene con `"videoRequests": { "enabled": false }`;
   dejarlo así:
   ```json
   "videoRequests": {
     "enabled": true,
     "workerOrigin": "https://videos.h0kd.dev"
   }
   ```
   Volver a abrir la app. Recién ahora aparece la pestaña **Video Requests**.
4. **Instalar lo que falte**: baja yt-dlp y ffmpeg (~110 MB). El botón dice
   "Bajando…" hasta que termina.
5. **Emparejar**: generar el código en `/admin?ch=<login>` → Ajustes, y
   pegarlo en la app. Dura 10 minutos y sirve una sola vez. Al quedar en
   "conectado · <login>", listo.
6. **OBS**: fuente de navegador con la URL que la app muestra abajo
   ("OBS Browser Source"), al tamaño del canvas. La experimental usa el
   puerto 3002.
7. **En pantalla** (misma pestaña): posición, tamaño y volumen de los
   pedidos. Se aplica con Guardar.

Cookies de Instagram: no hacen falta para arrancar. Solo si los reels
empiezan a fallar por login (la app muestra "VENCIDAS" o "sin cargar").

## Para probar sin estar en vivo

En `/admin` → Ajustes, la casilla **Envíos abiertos**. El próximo evento de
stream (online/offline) vuelve a mandar.
