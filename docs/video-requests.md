# Video Requests

Tus viewers mandan links de videos, tus mods los aprueban desde una página web
y lo aprobado sale en tu overlay de OBS, en orden y sin pisar los canjes.
Es un módulo **opcional** de Stream Overlay: viene apagado y, apagado, la app
funciona igual que siempre.

> **Beta con alta manual.** Para que funcione, tu canal tiene que estar
> habilitado en el servidor de h0kd (`videos.h0kd.dev`). Por ahora eso lo
> hacemos nosotros a mano, uno por uno. Pedilo abriendo un
> [issue en GitHub](https://github.com/h0kd/h0kd-overlay/issues/new) con tu
> nombre de canal de Twitch, o escribiéndole a h0kd directo. Cuando esté,
> seguí los pasos de abajo.

## Cómo funciona

```
viewer manda link  →  videos.h0kd.dev  →  un mod aprueba  →  tu app baja el video  →  overlay de OBS
```

- **Viewers**: entran a `https://videos.h0kd.dev/submit?ch=<tu_canal>` con su
  cuenta de Twitch y pegan un link. Solo mientras estás en vivo.
- **Mods**: en `https://videos.h0kd.dev/mod?ch=<tu_canal>` ven la cola con
  miniatura y duración, y aprueban o rechazan. Nada se descarga hasta que un
  humano aprueba.
- **Tu app**: baja el video aprobado con yt-dlp, lo prepara y lo reproduce en
  el overlay cuando la pantalla está libre. Los canjes de Channel Points
  nunca esperan: siguen saliendo al instante.
- **Vos**: administrás todo desde `https://videos.h0kd.dev/admin?ch=<tu_canal>`.

**Plataformas aceptadas:** YouTube (videos y Shorts), TikTok, Instagram
(solo Reels), X (posts con video), clips de Twitch y archivos de kappa.lol.
Cualquier otro link se rechaza al enviarlo, con el motivo.

## Puesta en marcha (una sola vez)

### 1. En la web: dar de alta tu canal

1. Entrá a `https://videos.h0kd.dev/admin` con **tu cuenta de Twitch** (la del
   canal). Eso registra el canal y suscribe los eventos de stream online /
   offline: los envíos se abren y cierran solos con tu stream. Quedás en
   `/admin?ch=<tu_canal>`; guardá ese link.
2. Pestaña **Moderadores**: marcá a quién le das acceso al panel de mods. Ser
   mod en Twitch no alcanza; hay que marcarlo acá.
3. Pestaña **Ajustes**: ahí están los links para compartir (viewers y mods) y
   la política de envío: cooldown por usuario, máximo en cola por usuario,
   duración máxima, tamaño y resolución máximos, gap entre videos.

### 2. En la app: activar el módulo

1. Instalá la [última release](https://github.com/h0kd/h0kd-overlay/releases/latest)
   (0.3.0 o más nueva) y abrila.
2. Pestaña **Video Requests**, marcá **Activar módulo**, tocá **Guardar** y
   cerrá y volvé a abrir la app. El módulo arranca al iniciar.
3. **Instalar lo que falte**: baja yt-dlp y ffmpeg (unos 110 MB) a la carpeta
   de datos de la app. El botón dice "Bajando…" hasta que termina.
4. **Emparejar**: en `/admin?ch=<tu_canal>`, pestaña Ajustes, **Generar código**.
   Pegalo en la app y tocá **Emparejar**. El código dura 10 minutos y sirve
   una sola vez. Cuando dice **conectado · tu_canal**, listo.

### 3. En OBS

Nada nuevo si ya usás la app: los pedidos salen por el mismo Browser Source
del overlay, `http://localhost:3001/overlay`. La URL exacta está en la barra
de abajo de la app.

### 4. En pantalla

En la misma pestaña, la tarjeta **En pantalla** define cómo salen los
pedidos, aparte de los canjes: posición fija o al azar respetando safe zones,
tamaño, volumen, animaciones de entrada y salida, y si se muestra
**quién lo pidió** arriba del video. Se aplica al tocar **Guardar**.

## Compartir los links

En `/admin?ch=<tu_canal>`, pestaña Ajustes, tenés los dos links con botón **Copiar**:

| Para | Link |
|------|------|
| Viewers | `https://videos.h0kd.dev/submit?ch=<tu_canal>` |
| Mods | `https://videos.h0kd.dev/mod?ch=<tu_canal>` |

Un comando de chat o un panel del canal con el link de viewers alcanza.

## Durante el stream

- Los envíos se **abren solos** cuando tu stream pasa a online y se cierran
  al terminar (con unos 45 segundos de gracia por si se te cae internet un
  momento, para no vaciarte la cola).
- El viewer ve su pedido pasar por **en revisión, aprobado, reproducido**
  (o rechazado, con motivo) en "Mis envíos".
- Los mods reciben un aviso en la página cuando entra algo nuevo.
- Si un link no se puede bajar (video borrado, privado, restringido), el
  pedido se rechaza solo con el motivo, sin que nadie tenga que tocar nada.

Para **probar sin estar en vivo**: `/admin`, pestaña Ajustes, casilla
**Envíos abiertos**. El próximo evento de stream lo vuelve a poner automático.

## Ventana de preview (VRChat)

Si streameás desde VRChat y no ves OBS, marcá **Ventana de preview (para
VRChat)** en la pestaña Video Requests. Se abre una ventana aparte de la app
que muestra los pedidos **sin sonido** a la vez que salen en OBS; capturala
con XSOverlay. Actúa al instante, se abre sola al iniciar la app y cerrarla
con la X la desmarca.

## Cookies de Instagram (opcional)

No hacen falta para arrancar. Algunos reels (restringidos por edad o región)
no se pueden bajar sin sesión, y si Instagram empieza a pedir login la app
lo muestra en **Entorno** ("VENCIDAS" o "sin cargar").

Si querés que esos reels anden:

1. Creá una cuenta de Instagram **dedicada**. Nunca uses la personal: Instagram
   puede bloquear la cuenta que use yt-dlp.
2. Iniciá sesión con esa cuenta en el navegador y exportá las cookies en
   formato Netscape (extensiones como "Get cookies.txt LOCALLY").
3. En la app: **Abrir carpeta de cookies** y guardá el archivo como
   `instagram-cookies.txt`.

## Problemas comunes

| Problema | Qué mirar |
|----------|-----------|
| `/admin` dice que el canal "todavía no está habilitado" | Tu canal no está dado de alta en el servidor. Pedilo (ver arriba). |
| "Emparejar" falla | El código dura 10 minutos y sirve una vez: generá otro. Revisá que el módulo diga **activo** (si pide reiniciar, cerrá y abrí la app). |
| El pedido queda en "leyendo el video…" | La app no está abierta o no está conectada. En la pestaña Video Requests tiene que decir **conectado**. Si no, **Reconectar**. |
| Nadie puede enviar | Los envíos se abren solos al estar en vivo. Para probar, activá **Envíos abiertos** en Ajustes. |
| Un mod no ve la cola | Hay que marcarlo en `/admin`, pestaña Moderadores; ser mod de Twitch no alcanza. |
| Reel de Instagram rechazado por restringido | Necesita cookies de una cuenta dedicada (ver arriba). |
| YouTube dice que yt-dlp está viejo | **Actualizar yt-dlp** en la tarjeta Entorno. |
| El video sale pero sin sonido | Volumen en la tarjeta En pantalla, y el audio del Browser Source en OBS. |

Los logs de la app quedan en la carpeta de datos, en `logs/`. El botón
**Carpeta** de la app la abre. Si algo no cierra, abrí un
[issue](https://github.com/h0kd/h0kd-overlay/issues) con el log del día.

## Venís de "Stream Overlay Experimental"

Si usaste la app experimental (exp-v0.2.10 a exp-v0.2.13), la 0.3.0 hace la
mudanza sola la primera vez que abre: copia el emparejamiento, yt-dlp, ffmpeg,
las cookies y la preferencia de la ventana de preview desde
`%APPDATA%\Stream Overlay Experimental\` a `%APPDATA%\Stream Overlay\`, y
trae la configuración del módulo (incluido si estaba activado). No toca la
carpeta vieja. Después:

1. Cerrá la app experimental antes de abrir la 0.3.0 (las dos no pueden estar
   emparejadas a la vez; la nube se queda con la última que conecta).
2. En OBS, el Browser Source vuelve al puerto **3001**:
   `http://localhost:3001/overlay`.
3. Desinstalá "Stream Overlay Experimental" cuando quieras. No se va a
   actualizar más.

Las rewards y videos no se migran: la app estable ya tenía los tuyos. Si
los editaste en la experimental después del 2026-09-03, copiá `config.json`
a mano.
